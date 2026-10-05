pub mod http;
pub mod aggregation_archive;

pub fn source_revision() -> String { env!("EUDR_SOURCE_REVISION").to_string() }
pub fn compute_host() -> Result<String,String> {
    let output=std::process::Command::new("hostname").output().map_err(|e|e.to_string())?;
    if !output.status.success() { return Err("cannot read compute hostname".into()); }
    let host=String::from_utf8(output.stdout).map_err(|e|e.to_string())?.trim().to_string();
    if host.is_empty() { return Err("compute hostname is empty".into()); }
    Ok(host)
}
/// Trusted configuration is loaded before any measurement/proof request.
pub fn require_registry_configuration(registry:&eudr_policy::registry::RegistrySnapshot,leaf_vkey:&str,epoch_vkey:&str,leaf_digest:[u8;32],epoch_digest:[u8;32]) -> Result<(),String> {
    let path=std::env::var("POLICY_MANIFEST").map_err(|_|"POLICY_MANIFEST required")?;
    let value:serde_json::Value=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if value["protocolVersion"]!=2 { return Err("policy manifest must be v2".into()); }
    let root=format!("0x{}",hex::encode(registry.root()));
    let polygon=format!("0x{}",hex::encode(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce)));
    let pairs=value["acceptedPolicies"].as_array().cloned().unwrap_or_else(||vec![value.clone()]);
    if !pairs.iter().any(|p|p["certificateRoot"].as_str().is_some_and(|v|v.eq_ignore_ascii_case(&root)) && p["polygonCommitment"].as_str().is_some_and(|v|v.eq_ignore_ascii_case(&polygon))) { return Err("workload policy roots absent from trusted manifest".into()); }
    for (field,expected) in [("leafProgramVKey",leaf_vkey.to_string()),("aggregateProgramVKey",epoch_vkey.to_string()),("leafVKeyDigest",format!("0x{}",hex::encode(leaf_digest))),("aggregateVKeyDigest",format!("0x{}",hex::encode(epoch_digest)))] {
        if !value[field].as_str().is_some_and(|v|v.eq_ignore_ascii_case(&expected)) { return Err(format!("trusted manifest {field} mismatch")); }
    }
    Ok(())
}


/// Production private key/nonces must use OS entropy; seeded helpers below are synthetic only.
pub fn write_private_archive(path:&std::path::Path,bytes:&[u8])->Result<(),String> {
    use std::io::Write;let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    options.open(path).and_then(|mut f|f.write_all(bytes)).map_err(|e|e.to_string())
}
pub fn random_private_bytes() -> Result<[u8;32],String> {
    let mut bytes=[0;32]; getrandom::getrandom(&mut bytes).map_err(|_| "OS entropy unavailable".to_string())?; Ok(bytes)
}


use ed25519_dalek::{Signer, SigningKey};
pub use eudr_policy::EventWitness;
use eudr_policy::{
    evaluate,
    nullifier_default_path, nullifier_default_root, EpcisEventV1, PointE6, PolicyError,
    PolicyInput, CERTIFICATE_TREE_DEPTH, MAX_EVENTS,
};
use sha2::{Digest, Sha256};
use sp1_sdk::{
    blocking::{ProveRequest, Prover, ProverClient},
    include_elf, Elf, HashableKey, ProvingKey, SP1Proof, SP1ProofWithPublicValues, SP1Stdin,
};

pub const EUDR_POLICY_ELF: Elf = include_elf!("eudr-policy-program");
pub const EUDR_EPOCH_ELF: Elf = include_elf!("eudr-epoch-program");

/// Synthetic but canonical signed events for one benchmark lot.
/// The seed only derives fixture keys; every proof still verifies the actual Ed25519 signatures.
pub fn benchmark_events_for_lot(
    seed: u64,
    epoch_id: u64,
    lot_id: u64,
    first_event_id: u64,
    event_count: usize,
) -> Result<Vec<EventWitness>, String> {
    if !(eudr_policy::MIN_EVENTS..=MAX_EVENTS).contains(&event_count) {
        return Err(format!(
            "benchmark lot size {event_count} is outside 8..=64"
        ));
    }

    (0..event_count)
        .map(|offset| {
            let event_id = first_event_id + offset as u64;
            let mut key_material = Sha256::new();
            key_material.update(b"EUDR:E2:BENCHMARK:ED25519:V1");
            key_material.update(seed.to_be_bytes());
            key_material.update(lot_id.to_be_bytes());
            let signing_key = SigningKey::from_bytes(&key_material.finalize().into());
            let event = EpcisEventV1 {
                event_id,
                lot_id,
                epoch_id,
                timestamp_ms: 1_700_000_000_000 + (first_event_id % 1_000_000_000) * 15_000 + offset as u64 * 15_000,
                readings: 800 + (event_id % 101) as u32,
                // C1 proves the event is outside the committed forbidden polygon.
                latitude_e6: 10_830_000 + (offset % 100) as i32,
                longitude_e6: 106_760_000 + (offset % 100) as i32,
                certificate_id: lot_id,
                role: eudr_policy::REQUIRED_ROLE,
                actor_public_key: signing_key.verifying_key().to_bytes(),
            };
            let event_bytes = event.canonical_bytes();
            Ok(EventWitness {
                event_bytes,
                signature: signing_key.sign(&event_bytes).to_bytes(),
                credential_index: 0,
                certificate_path: [[0_u8; 32]; CERTIFICATE_TREE_DEPTH],
            })
        })
        .collect()
}

/// Synthetic fixtures only. Prebuild once for the whole logical workload, before timing.
pub fn benchmark_registry(seed: u64, events: &[EventWitness]) -> Result<eudr_policy::registry::RegistrySnapshot, String> {
    use eudr_policy::registry::{Credential, RegistryIssuer};
    let mut credentials = Vec::new();
    let mut identities = std::collections::HashSet::new();
    for witness in events {
        let event = eudr_policy::decode_event(&witness.event_bytes).map_err(policy_error)?;
        if identities.insert((event.lot_id,event.actor_public_key,event.certificate_id,event.role)) {
            let index = u16::try_from(credentials.len()).map_err(|_| "registry capacity exceeded")?;
            credentials.push(Credential { index, event, owner_identity: benchmark_owner(seed),
                key_commitment: eudr_policy::nullifier_key_commitment(benchmark_nullifier_key(seed,event.lot_id)) });
        }
    }
    RegistryIssuer::default().snapshot(credentials, forbidden_polygon(), synthetic_nonce(seed,0,b"POLYGON")).map_err(policy_error)
}
pub fn benchmark_owner(seed: u64) -> [u8;32] { synthetic_nonce(seed,0,b"OWNER") }
fn synthetic_nonce(seed: u64, lot: u64, domain: &[u8]) -> [u8;32] {
    let mut h = Sha256::new(); h.update(b"EUDR:SYNTHETIC:V2"); h.update(domain); h.update(seed.to_be_bytes()); h.update(lot.to_be_bytes()); h.finalize().into()
}
/// Credential issuance is deliberately absent here: membership comes from the prebuilt issuer snapshot.
pub fn policy_input_from_events(
    epoch_id: u64, lot_id: u64, mut events: Vec<EventWitness>, old_nullifier_root: [u8;32],
    nullifier_key: [u8;32], nullifier_path: [[u8;32];eudr_policy::NULLIFIER_TREE_DEPTH],
    registry: &eudr_policy::registry::RegistrySnapshot, owner_identity: [u8;32], batch_nonce: [u8;32],
) -> Result<PolicyInput,String> {
    for witness in &mut events {
        let event = eudr_policy::decode_event(&witness.event_bytes).map_err(policy_error)?;
        if event.epoch_id != epoch_id || event.lot_id != lot_id { return Err("event assigned to wrong lot/epoch".into()); }
        let (index,path) = registry.credential(&event,owner_identity,nullifier_key).map_err(policy_error)?;
        witness.credential_index=index; witness.certificate_path=path;
    }
    Ok(PolicyInput { epoch_id,lot_id,polygon:registry.polygon.clone(),certificate_root:registry.root(),old_nullifier_root,
        nullifier_key,owner_identity,polygon_nonce:registry.polygon_nonce,batch_nonce,nullifier_path,events })
}
pub fn benchmark_batch_nonce(seed:u64,lot:u64) -> [u8;32] { synthetic_nonce(seed,lot,b"BATCH") }

pub fn benchmark_nullifier_key(seed: u64, lot_id: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"EUDR:E2:BENCHMARK:NULLIFIER-KEY:V2");
    hasher.update(seed.to_be_bytes());
    hasher.update(lot_id.to_be_bytes());
    hasher.finalize().into()
}

pub fn valid_input() -> PolicyInput {
    fixture_input(20260823, 1)
}

/// Produces one valid eight-event lot with caller-controlled public identifiers.
/// The E2E smoke uses unique identifiers so Fabric rejects accidental replays.
pub fn fixture_input(epoch_id: u64, first_event_id: u64) -> PolicyInput {
    let events = benchmark_events_for_lot(7,epoch_id,17,first_event_id,8).unwrap();
    let registry = benchmark_registry(7,&events).unwrap();
    policy_input_from_events(epoch_id,17,events,nullifier_default_root(),benchmark_nullifier_key(7,17),nullifier_default_path(),
        &registry,benchmark_owner(7),benchmark_batch_nonce(7,17)).unwrap()
}

fn forbidden_polygon() -> Vec<PointE6> {
    vec![
        PointE6 {
            latitude_e6: 10_760_000,
            longitude_e6: 106_660_000,
        },
        PointE6 {
            latitude_e6: 10_780_000,
            longitude_e6: 106_650_000,
        },
        PointE6 {
            latitude_e6: 10_800_000,
            longitude_e6: 106_680_000,
        },
        PointE6 {
            latitude_e6: 10_785_000,
            longitude_e6: 106_720_000,
        },
        PointE6 {
            latitude_e6: 10_765_000,
            longitude_e6: 106_710_000,
        },
    ]
}

pub fn prove(input: &PolicyInput) -> Result<SP1ProofWithPublicValues, String> {
    Ok(prove_internal(input)?.0)
}

pub fn prove_evm_fixture(input: &PolicyInput) -> Result<String, String> {
    let (proof, program_vkey) = prove_internal(input)?;
    Ok(serde_json::json!({"protocolVersion":2,"sourceRevision":source_revision(),"verificationKind":"sp1-groth16","vkey":program_vkey,
        "publicValues":format!("0x{}",hex::encode(proof.public_values.as_slice())),"proof":format!("0x{}",hex::encode(proof.bytes()))}).to_string())
}

/// Initialisation is deliberately outside the timed proof window in E2.
/// The same proving key is reused for every independently proven lot.
pub fn setup_policy_prover<P: Prover>(client: &P) -> Result<P::ProvingKey, String> {
    client
        .setup(EUDR_POLICY_ELF)
        .map_err(|error| format!("SP1 policy setup failed: {error}"))
}

pub fn setup_epoch_prover<P: Prover>(client: &P) -> Result<P::ProvingKey, String> {
    client
        .setup(EUDR_EPOCH_ELF)
        .map_err(|error| format!("SP1 epoch setup failed: {error}"))
}

pub fn prove_policy_compressed_with<P: Prover>(
    client: &P,
    key: &P::ProvingKey,
    input: &PolicyInput,
) -> Result<SP1ProofWithPublicValues, String> {
    let expected = evaluate(input).map_err(policy_error)?;
    let mut stdin = SP1Stdin::new();
    stdin.write(&input.encode().map_err(policy_error)?);
    let proof = client
        .prove(key, stdin)
        .compressed()
        .run()
        .map_err(|error| format!("SP1 compressed leaf proving failed: {error}"))?;
    client
        .verify(&proof, key.verifying_key(), None)
        .map_err(|error| format!("SP1 compressed leaf verification failed: {error}"))?;
    if proof.public_values.as_slice() != expected.abi_encode() {
        return Err("compressed leaf public values disagree with policy evaluation".to_string());
    }
    Ok(proof)
}

pub fn prove_epoch_with<P: Prover>(
    client: &P,
    epoch_key: &P::ProvingKey,
    leaf_key: &P::ProvingKey,
    children: Vec<SP1ProofWithPublicValues>,
) -> Result<SP1ProofWithPublicValues, String> {
    prove_epoch_node(client, epoch_key, leaf_key, children.into_iter().map(|p| (false,p)).collect(), false)
}

/// Recursively reduce arbitrary ordered leaf proofs with fan-in at most eight.
pub fn prove_epoch_tree_with<P: Prover>(
    client: &P,
    epoch_key: &P::ProvingKey,
    leaf_key: &P::ProvingKey,
    children: Vec<SP1ProofWithPublicValues>,
) -> Result<SP1ProofWithPublicValues, String> {
    if children.is_empty() {
        return Err("epoch has no children".to_string());
    }
    let mut children: Vec<_> = children.into_iter().map(|p| (false,p)).collect();
    while children.len() > eudr_policy::epoch::MAX_FAN_IN {
        let mut next = Vec::new();
        let mut remaining = children.into_iter();
        loop {
            let group: Vec<_> = remaining
                .by_ref()
                .take(eudr_policy::epoch::MAX_FAN_IN)
                .collect();
            if group.is_empty() {
                break;
            }
            next.push((true,prove_epoch_node(client, epoch_key, leaf_key, group, true)?));
        }
        children = next;
    }
    prove_epoch_node(client, epoch_key, leaf_key, children, false)
}

fn prove_epoch_node<P: Prover>(
    client: &P,
    epoch_key: &P::ProvingKey,
    leaf_key: &P::ProvingKey,
    children: Vec<(bool, SP1ProofWithPublicValues)>,
    compressed: bool,
) -> Result<SP1ProofWithPublicValues, String> {
    use eudr_policy::epoch::{EpochChild, LEAF_PUBLIC_BYTES};
    let mut public_children = Vec::with_capacity(children.len());
    for (aggregate, proof) in &children {
        let values = proof.public_values.as_slice();
        if values.len() != LEAF_PUBLIC_BYTES { return Err("child ABI must be exactly 352 bytes".into()); }
        let (vk, child) = if *aggregate {
            (epoch_key.verifying_key(), EpochChild::Aggregate(values.try_into().unwrap()))
        } else { (leaf_key.verifying_key(), EpochChild::Leaf(values.try_into().unwrap())) };
        client
            .verify(proof, vk, None)
            .map_err(|error| format!("SP1 child verification failed: {error}"))?;
        public_children.push(child);
    }
    let input = eudr_policy::epoch::EpochInput {
        leaf_vk_digest: leaf_key.verifying_key().hash_bytes(),
        aggregate_vk_digest: epoch_key.verifying_key().hash_bytes(),
        children: public_children,
    };
    let expected =
        eudr_policy::epoch::evaluate(&input).map_err(|error| format!("invalid epoch: {error}"))?;
    let mut stdin = SP1Stdin::new();
    stdin.write(&input.encode().map_err(str::to_string)?);
    for (aggregate, proof) in children {
        let vk = if aggregate { epoch_key.verifying_key() } else { leaf_key.verifying_key() };
        let SP1Proof::Compressed(compressed) = proof.proof else {
            return Err("epoch aggregation requires compressed SP1 child proofs".to_string());
        };
        stdin.write_proof(*compressed, vk.vk.clone());
    }
    let proof = if compressed {
        client.prove(epoch_key, stdin).compressed().run()
    } else {
        client.prove(epoch_key, stdin).groth16().run()
    }
    .map_err(|error| format!("SP1 epoch proving failed: {error}"))?;
    client
        .verify(&proof, epoch_key.verifying_key(), None)
        .map_err(|error| format!("SP1 epoch local verification failed: {error}"))?;
    if proof.public_values.as_slice() != expected.abi_encode() {
        return Err("epoch proof public values disagree with authenticated leaves".to_string());
    }
    Ok(proof)
}

pub fn prove_with<P: Prover>(
    client: &P,
    proving_key: &P::ProvingKey,
    input: &PolicyInput,
) -> Result<SP1ProofWithPublicValues, String> {
    let expected = evaluate(input).map_err(policy_error)?;
    let mut stdin = SP1Stdin::new();
    stdin.write(&input.encode().map_err(policy_error)?);
    let proof = client
        .prove(proving_key, stdin)
        .groth16()
        .run()
        .map_err(|error| format!("SP1 Groth16 proving failed: {error}"))?;
    client
        .verify(&proof, proving_key.verifying_key(), None)
        .map_err(|error| format!("SP1 local verification failed: {error}"))?;
    if proof.public_values.as_slice() != expected.abi_encode() {
        return Err("guest public values do not match host policy evaluation".to_string());
    }
    Ok(proof)
}

pub fn prove_evm_fixture_with<P: Prover>(
    client: &P,
    proving_key: &P::ProvingKey,
    input: &PolicyInput,
) -> Result<String, String> {
    let proof = prove_with(client, proving_key, input)?;
    Ok(serde_json::json!({"protocolVersion":2,"sourceRevision":source_revision(),"verificationKind":"sp1-groth16","vkey":proving_key.verifying_key().bytes32(),
        "publicValues":format!("0x{}",hex::encode(proof.public_values.as_slice())),"proof":format!("0x{}",hex::encode(proof.bytes()))}).to_string())
}

fn prove_internal(input: &PolicyInput) -> Result<(SP1ProofWithPublicValues, String), String> {
    let client = ProverClient::builder().cpu().build();
    let proving_key = setup_policy_prover(&client)?;
    let proof = prove_with(&client, &proving_key, input)?;
    Ok((proof, proving_key.verifying_key().bytes32().to_string()))
}

fn policy_error(error: PolicyError) -> String {
    format!("policy rejected fixture: {error:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp1_sdk::blocking::Prover;

    #[test]
    fn fixture_satisfies_all_c1_to_c5_predicates() {
        let input = valid_input();
        let public_values = evaluate(&input).unwrap();
        assert_eq!(public_values.epoch_id, input.epoch_id);
        assert_eq!(public_values.old_nullifier_root, nullifier_default_root());
        assert_eq!(public_values.event_count, 8);
        assert_eq!(
            PolicyInput::decode(&input.encode().unwrap()).unwrap(),
            input
        );
    }

    #[test]
    fn fixture_accepts_caller_controlled_epoch_and_event_ids() {
        let input = fixture_input(2_026_082_401, 9_000_000_100);
        let public_values = evaluate(&input).unwrap();
        assert_eq!(public_values.epoch_id, 2_026_082_401);
        assert_eq!(
            input.events[0].event_bytes[1..9],
            9_000_000_100_u64.to_be_bytes()
        );
    }

    #[test]
    fn benchmark_lot_binds_all_canonical_events_to_the_policy_input() {
        let events = benchmark_events_for_lot(7, 70, 71, 7_000, 16).unwrap();
        let registry = benchmark_registry(7,&events).unwrap();
        let input = policy_input_from_events(
            70,
            71,
            events,
            nullifier_default_root(),
            benchmark_nullifier_key(7, 71),
            nullifier_default_path(),
            &registry,benchmark_owner(7),benchmark_batch_nonce(7,71),
        )
        .unwrap();
        assert_eq!(input.events.len(), 16);
        assert_eq!(evaluate(&input).unwrap().event_count, 16);
    }

    #[test]
    fn fixture_rejects_a_tampered_signed_event() {
        let mut input = valid_input();
        input.events[0].event_bytes[52] ^= 1;
        assert_eq!(evaluate(&input), Err(PolicyError::C4SignatureOrRole));
    }

    #[test]
    fn fixture_rejects_each_policy_tamper() {
        let mut inside_polygon = valid_input();
        inside_polygon.polygon = vec![
            PointE6 {
                latitude_e6: 10_820_000,
                longitude_e6: 106_750_000,
            },
            PointE6 {
                latitude_e6: 10_820_000,
                longitude_e6: 106_770_000,
            },
            PointE6 {
                latitude_e6: 10_840_000,
                longitude_e6: 106_770_000,
            },
            PointE6 {
                latitude_e6: 10_840_000,
                longitude_e6: 106_750_000,
            },
        ];
        assert_eq!(evaluate(&inside_polygon), Err(PolicyError::C1Geofence));

        let mut wrong_certificate_root = valid_input();
        wrong_certificate_root.certificate_root[0] ^= 1;
        assert_eq!(
            evaluate(&wrong_certificate_root),
            Err(PolicyError::C2CredentialRoot)
        );

        let mut threshold_overflow = valid_input();
        threshold_overflow.events[0].event_bytes[35] = 0x03;
        threshold_overflow.events[0].event_bytes[36] = 0x85;
        assert_eq!(
            evaluate(&threshold_overflow),
            Err(PolicyError::C3ThresholdOrTime)
        );

        let mut replay = valid_input();
        replay.old_nullifier_root = evaluate(&replay).unwrap().new_nullifier_root;
        assert_eq!(evaluate(&replay), Err(PolicyError::C5Nullifier));
    }

    #[test]
    #[ignore = "HPC only: creates a real Groth16 proof"]
    fn cpu_groth16_proof_verifies_and_binds_public_values() {
        let input = valid_input();
        let expected = evaluate(&input).unwrap().abi_encode();
        let proof = prove(&input).unwrap();
        let client = ProverClient::builder().cpu().build();
        let proving_key = client.setup(EUDR_POLICY_ELF).unwrap();
        client
            .verify(&proof, proving_key.verifying_key(), None)
            .unwrap();
        assert_eq!(proof.public_values.as_slice(), expected);
    }
    #[test]
    fn key_binding_salts_openings_and_v2_decoding() {
        let input = valid_input(); let public = evaluate(&input).unwrap();
        let mut wrong = input.clone(); wrong.nullifier_key[0] ^= 1;
        assert_eq!(evaluate(&wrong), Err(PolicyError::C2CredentialRoot));
        let mut wrong = input.clone(); wrong.owner_identity[0] ^= 1;
        assert_eq!(evaluate(&wrong), Err(PolicyError::C2CredentialRoot));
        let tree = eudr_policy::event_tree(&input.events,input.batch_nonce);
        let index = 2; let mut slot=index;
        let path: Vec<_> = tree.iter().take(tree.len()-1).map(|level| { let s=level[slot^1];slot>>=1;s }).collect();
        let salt = eudr_policy::event_salt(input.batch_nonce,index);
        assert!(eudr_policy::verify_event_opening(8,index,salt,&input.events[index].event_bytes,&path,public.event_batch_digest));
        assert!(!eudr_policy::verify_event_opening(9,index,salt,&input.events[index].event_bytes,&path,public.event_batch_digest));
        let mut wrong_salt=salt;wrong_salt[0]^=1;
        assert!(!eudr_policy::verify_event_opening(8,index,wrong_salt,&input.events[index].event_bytes,&path,public.event_batch_digest));
        let mut changed=input.clone();changed.batch_nonce[0]^=1;
        assert_ne!(evaluate(&changed).unwrap().event_batch_digest,public.event_batch_digest);
        changed=input.clone();changed.polygon_nonce[0]^=1;
        assert_ne!(evaluate(&changed).unwrap().polygon_commitment,public.polygon_commitment);
        let mut bytes=input.encode().unwrap();bytes[0]=1;assert!(PolicyInput::decode(&bytes).is_err());
        bytes[0]=2;bytes.push(0);assert!(PolicyInput::decode(&bytes).is_err());
        let mut map=eudr_policy::SparseNullifierMap::default();
        let index=eudr_policy::nullifier_index(input.lot_id,input.nullifier_key);map.insert(index).unwrap();
        assert!(map.empty_path(index).is_err()); // index excludes epoch: cannot replay in a later epoch
    }

    #[test]
    fn two_lots_share_one_preissued_policy_snapshot() {
        let a=benchmark_events_for_lot(7,1,10,100,8).unwrap();
        let b=benchmark_events_for_lot(7,1,11,108,8).unwrap();
        let all:Vec<_>=a.iter().chain(&b).cloned().collect();
        let registry=benchmark_registry(7,&all).unwrap();
        let mut map=eudr_policy::SparseNullifierMap::default(); let mut children=Vec::new();
        for (lot,events) in [(10,a),(11,b)] {
            let key=benchmark_nullifier_key(7,lot);let index=eudr_policy::nullifier_index(lot,key);
            let input=policy_input_from_events(1,lot,events,map.root(),key,map.empty_path(index).unwrap(),&registry,benchmark_owner(7),benchmark_batch_nonce(7,lot)).unwrap();
            children.push(eudr_policy::epoch::EpochChild::Leaf(evaluate(&input).unwrap().abi_encode()));
            map.insert(index).unwrap();
        }
        let epoch=eudr_policy::epoch::evaluate(&eudr_policy::epoch::EpochInput { leaf_vk_digest:[1;32],aggregate_vk_digest:[2;32],children }).unwrap();
        assert_eq!(epoch.certificate_root,registry.root());assert_eq!(epoch.lot_count,2);assert_eq!(epoch.new_nullifier_root,map.root());
        let bytes=epoch.abi_encode();assert_eq!(bytes.len(),352);
        let nested=eudr_policy::epoch::evaluate(&eudr_policy::epoch::EpochInput {leaf_vk_digest:[1;32],aggregate_vk_digest:[2;32],children:vec![eudr_policy::epoch::EpochChild::Aggregate(bytes)]}).unwrap();
        assert_eq!(nested.certificate_root,registry.root());assert_eq!(nested.polygon_commitment,epoch.polygon_commitment);
    }

}
