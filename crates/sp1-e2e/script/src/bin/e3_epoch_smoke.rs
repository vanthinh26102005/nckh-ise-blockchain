use eudr_epoch_policy::{encode_aggregate_input, leaf_nullifier_id, LeafInput, LeafPublicValues};
use eudr_policy::{nullifier_index, SparseNullifierMap};
use sp1_e2e::{
    benchmark_events_for_lot, benchmark_nullifier_secret,
    epoch::{evm_fixture, prove_epoch_with, setup_epoch_provers, EPOCH_AGGREGATE_ELF},
    policy_input_from_events,
};
use sp1_sdk::{
    blocking::{Prover, ProverClient},
    ProvingKey, SP1Proof, SP1Stdin,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let epoch_id = 21;
    let shipment_id = 17;
    let seed = 21;
    let events_per_leaf: usize = std::env::var("E3_EVENTS_PER_LEAF")
        .ok()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(64);
    if !(8..=64).contains(&events_per_leaf) {
        return Err("E3_EVENTS_PER_LEAF must be 8..=64".into());
    }
    let prover = ProverClient::builder().cpu().build();
    let (leaf_key, aggregate_key) = setup_epoch_provers(&prover)?;
    let mut nullifiers = SparseNullifierMap::default();
    let mut leaves = Vec::new();
    for leaf_index in 0..2u8 {
        let nullifier_id = leaf_nullifier_id(epoch_id, shipment_id, leaf_index);
        let secret = benchmark_nullifier_secret(seed, nullifier_id);
        let index = nullifier_index(nullifier_id, secret);
        let events = benchmark_events_for_lot(
            seed,
            epoch_id,
            shipment_id,
            1_000 + u64::from(leaf_index) * events_per_leaf as u64,
            events_per_leaf,
        )?;
        let policy = policy_input_from_events(
            epoch_id,
            shipment_id,
            events,
            nullifiers.root(),
            secret,
            nullifiers
                .empty_path(index)
                .map_err(|e| format!("nullifier path: {e:?}"))?,
        )?;
        let leaf = LeafInput {
            shipment_id,
            leaf_index,
            leaf_count: 2,
            policy,
        };
        leaf.evaluate()
            .map_err(|e| format!("leaf evaluation: {e:?}"))?;
        nullifiers
            .insert(index)
            .map_err(|e| format!("nullifier insert: {e:?}"))?;
        leaves.push(leaf);
    }
    let epoch = prove_epoch_with(&prover, &leaf_key, &aggregate_key, &leaves)?;
    if epoch.values.new_nullifier_root != nullifiers.root()
        || epoch.values.event_count != (events_per_leaf * 2) as u32
        || epoch.values.lot_count != 1
    {
        return Err("aggregate output disagrees with the shipment".into());
    }
    let child_values = epoch
        .leaf_proofs
        .iter()
        .map(|proof| {
            LeafPublicValues::decode(proof.public_values.as_slice())
                .map_err(|error| format!("decode child public values: {error:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut tampered_values = child_values.clone();
    tampered_values[0].lot_commitment[0] ^= 1;
    // A valid proof for leaf 1 cannot stand in for leaf 0. Also reject changed
    // public values with the original proofs. Both checks execute the guest
    // with real deferred proof verification enabled.
    for (case, values, order) in [
        ("replaced child proof", child_values, [1usize, 1usize]),
        ("tampered child public values", tampered_values, [0, 1]),
    ] {
        let mut tampered = SP1Stdin::new();
        tampered.write(
            &encode_aggregate_input(&values)
                .map_err(|error| format!("encode {case}: {error:?}"))?,
        );
        for index in order {
            match &epoch.leaf_proofs[index].proof {
                SP1Proof::Compressed(compressed) => tampered
                    .write_proof((**compressed).clone(), leaf_key.verifying_key().vk.clone()),
                _ => return Err("expected compressed child proof".into()),
            }
        }
        if prover
            .execute(EPOCH_AGGREGATE_ELF, tampered)
            .calculate_gas(false)
            .run()
            .is_ok()
        {
            return Err(format!("aggregate guest accepted {case}").into());
        }
    }
    let path = std::env::var("E3_FIXTURE_OUT")
        .unwrap_or_else(|_| "/tmp/eudr-e3-epoch-fixture.json".to_string());
    std::fs::write(&path, evm_fixture(&epoch))?;
    println!("wrote 2-leaf epoch Groth16 fixture ({events_per_leaf} events/leaf) to {path}");
    Ok(())
}
