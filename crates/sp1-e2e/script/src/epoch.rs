//! E3 proof path. Every leaf proof is carried in SP1's deferred proof channel;
//! the aggregate guest verifies each proof before committing the epoch output.

use eudr_epoch_policy::{aggregate, encode_aggregate_input, AggregatePublicValues, LeafInput};
use sp1_sdk::{
    blocking::{ProveRequest, Prover},
    include_elf, Elf, HashableKey, ProvingKey, SP1Proof, SP1ProofWithPublicValues, SP1Stdin,
};
use std::time::Instant;

pub const EPOCH_LEAF_ELF: Elf = include_elf!("eudr-epoch-leaf-program");
pub const EPOCH_AGGREGATE_ELF: Elf = include_elf!("eudr-epoch-aggregate-program");

pub fn setup_epoch_provers<P: Prover>(
    client: &P,
) -> Result<(P::ProvingKey, P::ProvingKey), String> {
    let leaf = client
        .setup(EPOCH_LEAF_ELF)
        .map_err(|e| format!("leaf setup: {e}"))?;
    let aggregate = client
        .setup(EPOCH_AGGREGATE_ELF)
        .map_err(|e| format!("aggregate setup: {e}"))?;
    let embedded = env!("E3_LEAF_VKEY_WORDS");
    let actual = leaf
        .verifying_key()
        .hash_u32()
        .map(|w| w.to_string())
        .join(",");
    if actual != embedded {
        return Err("aggregate ELF embeds a different leaf vkey".into());
    }
    Ok((leaf, aggregate))
}

pub struct EpochProof {
    pub proof: SP1ProofWithPublicValues,
    pub leaf_proofs: Vec<SP1ProofWithPublicValues>,
    pub leaf_proving_ms: Vec<u128>,
    pub aggregate_proving_ms: u128,
    pub values: AggregatePublicValues,
    pub program_vkey: String,
}

pub fn prove_epoch_with<P: Prover>(
    client: &P,
    leaf_key: &P::ProvingKey,
    aggregate_key: &P::ProvingKey,
    inputs: &[LeafInput],
) -> Result<EpochProof, String> {
    if inputs.is_empty() || inputs.len() > eudr_epoch_policy::MAX_FAN_IN {
        return Err("epoch fan-in must be 1..=8".into());
    }
    let mut values = Vec::with_capacity(inputs.len());
    let mut proofs = Vec::with_capacity(inputs.len());
    let mut leaf_proving_ms = Vec::with_capacity(inputs.len());
    for input in inputs {
        let expected = input.evaluate().map_err(|e| format!("leaf input: {e:?}"))?;
        let mut stdin = SP1Stdin::new();
        stdin.write(
            &input
                .encode()
                .map_err(|e| format!("leaf encoding: {e:?}"))?,
        );
        let started = Instant::now();
        let proof = client
            .prove(leaf_key, stdin)
            .compressed()
            .run()
            .map_err(|e| format!("leaf proving: {e}"))?;
        leaf_proving_ms.push(started.elapsed().as_millis());
        client
            .verify(&proof, leaf_key.verifying_key(), None)
            .map_err(|e| format!("leaf local verify: {e}"))?;
        if proof.public_values.as_slice() != expected.encode() {
            return Err("leaf guest public values differ from the policy evaluation".into());
        }
        values.push(expected);
        proofs.push(proof);
    }
    let expected = aggregate(&values).map_err(|e| format!("epoch transition: {e:?}"))?;
    let mut stdin = SP1Stdin::new();
    stdin.write(&encode_aggregate_input(&values).map_err(|e| format!("aggregate input: {e:?}"))?);
    for proof in &proofs {
        match &proof.proof {
            SP1Proof::Compressed(compressed) => {
                stdin.write_proof((**compressed).clone(), leaf_key.verifying_key().vk.clone())
            }
            _ => return Err("leaf prover did not return a compressed proof".into()),
        }
    }
    let started = Instant::now();
    let proof = client
        .prove(aggregate_key, stdin)
        .groth16()
        .run()
        .map_err(|e| format!("aggregate Groth16 proving: {e}"))?;
    let aggregate_proving_ms = started.elapsed().as_millis();
    client
        .verify(&proof, aggregate_key.verifying_key(), None)
        .map_err(|e| format!("aggregate local verify: {e}"))?;
    if proof.public_values.as_slice() != expected.abi_encode() {
        return Err("aggregate guest public values differ from the transition".into());
    }
    Ok(EpochProof {
        proof,
        leaf_proofs: proofs,
        leaf_proving_ms,
        aggregate_proving_ms,
        values: expected,
        program_vkey: aggregate_key.verifying_key().bytes32(),
    })
}

pub fn evm_fixture(epoch: &EpochProof) -> String {
    format!(
        "{{\n  \"vkey\": \"{}\",\n  \"publicValues\": \"0x{}\",\n  \"proof\": \"0x{}\"\n}}\n",
        epoch.program_vkey,
        hex::encode(epoch.proof.public_values.as_slice()),
        hex::encode(epoch.proof.bytes())
    )
}
