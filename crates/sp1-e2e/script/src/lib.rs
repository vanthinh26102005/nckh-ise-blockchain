use ed25519_dalek::{Signer, SigningKey};
use eudr_policy::{
    certificate_leaf, certificate_padding_leaf, certificate_parent, evaluate,
    nullifier_default_path, nullifier_default_root, EpcisEventV1, EventWitness, PointE6,
    PolicyError, PolicyInput, CERTIFICATE_TREE_DEPTH, MAX_EVENTS,
};
use sp1_sdk::{
    blocking::{ProveRequest, Prover, ProverClient},
    include_elf, Elf, HashableKey, ProvingKey, SP1ProofWithPublicValues, SP1Stdin,
};

pub const EUDR_POLICY_ELF: Elf = include_elf!("eudr-policy-program");

pub fn valid_input() -> PolicyInput {
    fixture_input(20260823, 1)
}

/// Produces one valid eight-event lot with caller-controlled public identifiers.
/// The E2E smoke uses unique identifiers so Fabric rejects accidental replays.
pub fn fixture_input(epoch_id: u64, first_event_id: u64) -> PolicyInput {
    let lot_id = 17;
    let polygon = vec![
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
    ];
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let mut decoded_events = Vec::new();
    let mut signatures = Vec::new();
    for index in 0..8_u64 {
        let event = EpcisEventV1 {
            event_id: first_event_id + index,
            lot_id,
            epoch_id,
            timestamp_ms: 1_700_000_000_000 + index * 15_000,
            readings: 800 + index as u32,
            latitude_e6: 10_830_000 + index as i32,
            longitude_e6: 106_760_000 + index as i32,
            certificate_id: 10_000 + index,
            role: 4,
            actor_public_key: signing_key.verifying_key().to_bytes(),
        };
        let event_bytes = event.canonical_bytes();
        decoded_events.push(event);
        signatures.push((event_bytes, signing_key.sign(&event_bytes).to_bytes()));
    }

    let (certificate_root, certificate_paths) = certificate_tree(&decoded_events);
    let events = signatures
        .into_iter()
        .enumerate()
        .map(|(index, (event_bytes, signature))| EventWitness {
            event_bytes,
            signature,
            certificate_path: certificate_paths[index],
        })
        .collect();

    PolicyInput {
        epoch_id,
        lot_id,
        polygon,
        certificate_root,
        old_nullifier_root: nullifier_default_root(),
        nullifier_secret: [19_u8; 32],
        nullifier_path: nullifier_default_path(),
        events,
    }
}

pub fn prove(input: &PolicyInput) -> Result<SP1ProofWithPublicValues, String> {
    Ok(prove_internal(input)?.0)
}

pub fn prove_evm_fixture(input: &PolicyInput) -> Result<String, String> {
    let (proof, program_vkey) = prove_internal(input)?;
    Ok(format!(
        "{{\n  \"vkey\": \"{program_vkey}\",\n  \"publicValues\": \"0x{}\",\n  \"proof\": \"0x{}\"\n}}\n",
        hex::encode(proof.public_values.as_slice()),
        hex::encode(proof.bytes())
    ))
}

fn prove_internal(input: &PolicyInput) -> Result<(SP1ProofWithPublicValues, String), String> {
    let expected = evaluate(input).map_err(policy_error)?;
    let mut stdin = SP1Stdin::new();
    stdin.write(&input.encode().map_err(policy_error)?);
    let client = ProverClient::builder().cpu().build();
    let proving_key = client
        .setup(EUDR_POLICY_ELF)
        .map_err(|error| format!("SP1 setup failed: {error}"))?;
    let proof = client
        .prove(&proving_key, stdin)
        .groth16()
        .run()
        .map_err(|error| format!("SP1 Groth16 proving failed: {error}"))?;
    client
        .verify(&proof, proving_key.verifying_key(), None)
        .map_err(|error| format!("SP1 local verification failed: {error}"))?;
    if proof.public_values.as_slice() != expected.abi_encode() {
        return Err("guest public values do not match host policy evaluation".to_string());
    }
    Ok((proof, proving_key.verifying_key().bytes32().to_string()))
}

fn certificate_tree(
    events: &[EpcisEventV1],
) -> ([u8; 32], Vec<[[u8; 32]; CERTIFICATE_TREE_DEPTH]>) {
    let mut levels = Vec::with_capacity(CERTIFICATE_TREE_DEPTH + 1);
    let mut current = (0..MAX_EVENTS)
        .map(|index| {
            if let Some(event) = events.get(index) {
                certificate_leaf(index, event)
            } else {
                certificate_padding_leaf(index)
            }
        })
        .collect::<Vec<_>>();
    levels.push(current.clone());
    while current.len() > 1 {
        current = current
            .chunks_exact(2)
            .map(|pair| certificate_parent(pair[0], pair[1]))
            .collect();
        levels.push(current.clone());
    }
    let paths = (0..events.len())
        .map(|event_index| {
            let mut path = [[0_u8; 32]; CERTIFICATE_TREE_DEPTH];
            let mut index = event_index;
            for (depth, level) in levels.iter().take(CERTIFICATE_TREE_DEPTH).enumerate() {
                path[depth] = level[index ^ 1];
                index >>= 1;
            }
            path
        })
        .collect();
    (current[0], paths)
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
}
