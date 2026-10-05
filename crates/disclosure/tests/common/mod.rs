use ed25519_dalek::{Signer, SigningKey};
use eudr_disclosure::{encode_hex, AuthenticationPath, ChildKind, Level, PublicChild};
use eudr_policy::{
    self as policy,
    epoch::{EpochChild, EpochInput},
    registry::{Credential, RegistryIssuer},
    EpcisEventV1, EventWitness, PointE6, PolicyInput,
};

pub fn fixture() -> (PolicyInput, AuthenticationPath) {
    let signing = SigningKey::from_bytes(&[3; 32]);
    let events = (0..8)
        .map(|index| {
            let event = EpcisEventV1 {
                event_id: index + 1,
                lot_id: 17,
                epoch_id: 7,
                timestamp_ms: index + 1,
                readings: 800,
                latitude_e6: 20,
                longitude_e6: 20,
                certificate_id: 3,
                role: 4,
                actor_public_key: signing.verifying_key().to_bytes(),
            };
            let bytes = event.canonical_bytes();
            EventWitness {
                event_bytes: bytes,
                signature: signing.sign(&bytes).to_bytes(),
                credential_index: 0,
                certificate_path: [[0; 32]; 16],
            }
        })
        .collect::<Vec<_>>();
    let owner = [7; 32];
    let key = [9; 32];
    let polygon = vec![PointE6::new(0, 0), PointE6::new(0, 10), PointE6::new(10, 0)];
    let credential = Credential {
        index: 0,
        event: policy::decode_event(&events[0].event_bytes).unwrap(),
        owner_identity: owner,
        key_commitment: policy::nullifier_key_commitment(key),
    };
    let registry = RegistryIssuer::default()
        .snapshot(vec![credential], polygon.clone(), [13; 32])
        .unwrap();
    let mut input = PolicyInput {
        epoch_id: 7,
        lot_id: 17,
        polygon,
        certificate_root: registry.root(),
        old_nullifier_root: policy::nullifier_default_root(),
        nullifier_key: key,
        owner_identity: owner,
        polygon_nonce: [13; 32],
        batch_nonce: [14; 32],
        nullifier_path: policy::nullifier_default_path(),
        events,
    };
    for witness in &mut input.events {
        let event = policy::decode_event(&witness.event_bytes).unwrap();
        let (index, path) = registry.credential(&event, owner, key).unwrap();
        witness.credential_index = index;
        witness.certificate_path = path;
    }
    let leaf = policy::evaluate(&input).unwrap().abi_encode();
    let path = AuthenticationPath {
        protocol_version: 2,
        transaction_hash: encode_hex([19; 32]),
        levels: vec![Level {
            leaf_v_key_digest: encode_hex([21; 32]),
            aggregate_v_key_digest: encode_hex([22; 32]),
            child_index: 0,
            children: vec![PublicChild {
                kind: ChildKind::Leaf,
                public_values: encode_hex(leaf),
            }],
        }],
    };
    assert!(policy::epoch::evaluate(&EpochInput {
        leaf_vk_digest: [21; 32],
        aggregate_vk_digest: [22; 32],
        children: vec![EpochChild::Leaf(leaf)]
    })
    .is_ok());
    (input, path)
}
