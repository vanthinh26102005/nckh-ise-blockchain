//! Public ordered authentication paths recorded while the real proof tree is built.
//! No private witness, proof bytes, key material or file IO is included here.
use eudr_policy::epoch::{EpochChild, EpochInput, MAX_FAN_IN};
use serde_json::{json, Value};
use sp1_sdk::{blocking::Prover, HashableKey, ProvingKey, SP1ProofWithPublicValues};

/// Append this public parent level to every original leaf below its selected child.
/// Descendant indexes reference the caller's original ordered leaf list.
fn append_level(
    paths: &mut [Value],
    input: &EpochInput,
    descendants: &[Vec<usize>],
) -> Result<Vec<usize>, String> {
    if input.children.len() != descendants.len() {
        return Err("archive descendants do not match public children".into());
    }
    eudr_policy::epoch::evaluate(input).map_err(str::to_string)?;
    let children = input.children.iter().map(|child| json!({
        "kind": match child { EpochChild::Leaf(_) => "leaf", EpochChild::Aggregate(_) => "aggregate" },
        "publicValues": format!("0x{}", hex::encode(child.bytes())),
    })).collect::<Vec<_>>();
    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Validate indexes before changing any archive entry.
    for leaves in descendants {
        if leaves.is_empty() {
            return Err("archive child has no original leaves".into());
        }
        for &leaf in leaves {
            if leaf >= paths.len()
                || !seen.insert(leaf)
                || paths[leaf]["levels"].as_array().is_none()
            {
                return Err("invalid or duplicate archive descendant index".into());
            }
        }
    }
    for (child_index, leaves) in descendants.iter().enumerate() {
        let level = json!({"leafVKeyDigest": format!("0x{}",hex::encode(input.leaf_vk_digest)),
            "aggregateVKeyDigest": format!("0x{}",hex::encode(input.aggregate_vk_digest)),
            "childIndex": child_index, "children": children});
        for &leaf in leaves {
            paths[leaf]["levels"]
                .as_array_mut()
                .unwrap()
                .push(level.clone());
            all.push(leaf);
        }
    }
    Ok(all)
}

/// The caller must attach the actual mined transactionHash before exporting paths.
/// Intermediate nodes use compressed proofs; the final root uses Groth16.
pub fn prove_epoch_tree_with_archive<P: Prover>(
    client: &P,
    epoch_key: &P::ProvingKey,
    leaf_key: &P::ProvingKey,
    leaves: Vec<SP1ProofWithPublicValues>,
) -> Result<(SP1ProofWithPublicValues, Vec<Value>), String> {
    if leaves.is_empty() {
        return Err("epoch has no children".into());
    }
    let mut paths = (0..leaves.len())
        .map(|_| json!({"protocolVersion":2,"transactionHash":null,"levels":[]}))
        .collect::<Vec<_>>();
    let mut nodes = leaves
        .into_iter()
        .enumerate()
        .map(|(index, proof)| (false, proof, vec![index]))
        .collect::<Vec<_>>();
    let leaf_vk_digest = leaf_key.verifying_key().hash_bytes();
    let aggregate_vk_digest = epoch_key.verifying_key().hash_bytes();
    loop {
        let intermediate = nodes.len() > MAX_FAN_IN;
        let mut remaining = nodes.into_iter();
        let mut next = Vec::new();
        loop {
            let group = remaining.by_ref().take(MAX_FAN_IN).collect::<Vec<_>>();
            if group.is_empty() {
                break;
            }
            let mut children = Vec::with_capacity(group.len());
            for (aggregate, proof, _) in &group {
                let bytes: [u8; 352] = proof
                    .public_values
                    .as_slice()
                    .try_into()
                    .map_err(|_| "archive child ABI must be exactly 352 bytes")?;
                children.push(if *aggregate {
                    EpochChild::Aggregate(bytes)
                } else {
                    EpochChild::Leaf(bytes)
                });
            }
            let input = EpochInput {
                leaf_vk_digest,
                aggregate_vk_digest,
                children,
            };
            let descendants = group
                .iter()
                .map(|(_, _, leaves)| leaves.clone())
                .collect::<Vec<_>>();
            let indexes = append_level(&mut paths, &input, &descendants)?;
            let proofs = group
                .into_iter()
                .map(|(aggregate, proof, _)| (aggregate, proof))
                .collect();
            let proof = super::prove_epoch_node(client, epoch_key, leaf_key, proofs, intermediate)?;
            if !intermediate {
                return Ok((proof, paths));
            }
            next.push((true, proof, indexes));
        }
        nodes = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn leaf(old: u8, new: u8, digest: u8) -> [u8; 352] {
        let mut values = [0; 352];
        values[31] = 7;
        values[32..64].fill(old);
        values[64..96].fill(new);
        values[96..128].fill(digest);
        values[128..160].fill(4);
        values[160..192].fill(5);
        values[255] = 4;
        values[284..288].copy_from_slice(&900_u32.to_be_bytes());
        values[319] = 8;
        values[351] = 1;
        values
    }
    #[test]
    fn archives_bottom_up_child_positions_for_all_original_leaves() {
        let mut paths = (0..3)
            .map(|_| json!({"protocolVersion":2,"transactionHash":null,"levels":[]}))
            .collect::<Vec<_>>();
        let left = EpochInput {
            leaf_vk_digest: [11; 32],
            aggregate_vk_digest: [12; 32],
            children: vec![
                EpochChild::Leaf(leaf(0, 1, 1)),
                EpochChild::Leaf(leaf(1, 2, 2)),
            ],
        };
        let right = EpochInput {
            children: vec![EpochChild::Leaf(leaf(2, 3, 3))],
            ..left.clone()
        };
        let left_indexes = append_level(&mut paths, &left, &[vec![0], vec![1]]).unwrap();
        let right_indexes = append_level(&mut paths, &right, &[vec![2]]).unwrap();
        let root = EpochInput {
            children: vec![
                EpochChild::Aggregate(eudr_policy::epoch::evaluate(&left).unwrap().abi_encode()),
                EpochChild::Aggregate(eudr_policy::epoch::evaluate(&right).unwrap().abi_encode()),
            ],
            ..left.clone()
        };
        assert_eq!(
            append_level(&mut paths, &root, &[left_indexes, right_indexes]).unwrap(),
            vec![0, 1, 2]
        );
        for path in &paths {
            assert_eq!(path["levels"].as_array().unwrap().len(), 2);
            assert!(path["transactionHash"].is_null());
        }
        assert_eq!(paths[0]["levels"][0]["childIndex"], 0);
        assert_eq!(paths[1]["levels"][0]["childIndex"], 1);
        assert_eq!(paths[2]["levels"][0]["childIndex"], 0);
        assert_eq!(paths[0]["levels"][1]["childIndex"], 0);
        assert_eq!(paths[2]["levels"][1]["childIndex"], 1);
        assert_eq!(paths[2]["levels"][1]["children"][0]["kind"], "aggregate");
        assert_eq!(
            paths[0]["levels"][0]["children"][1]["publicValues"],
            format!("0x{}", hex::encode(leaf(1, 2, 2)))
        );
        assert!(!serde_json::to_string(&paths).unwrap().contains("proof"));
    }
    #[test]
    fn rejects_invalid_descendant_indexes_without_partial_archive_updates() {
        let mut paths = vec![json!({"protocolVersion":2,"levels":[]})];
        let input = EpochInput {
            leaf_vk_digest: [11; 32],
            aggregate_vk_digest: [12; 32],
            children: vec![EpochChild::Leaf(leaf(0, 1, 1))],
        };
        assert!(append_level(&mut paths, &input, &[vec![1]]).is_err());
        assert!(append_level(&mut paths, &input, &[vec![0, 0]]).is_err());
        assert!(append_level(&mut paths, &input, &[]).is_err());
        assert!(paths[0]["levels"].as_array().unwrap().is_empty());
    }
}
