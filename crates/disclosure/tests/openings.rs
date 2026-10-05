use eudr_disclosure::{parse_fields, Selection};

#[test]
fn explicit_selection_and_lot_binding_are_required() {
    let selected = parse_fields("polygon,events[0..3],credential", 8).unwrap();
    assert_eq!(
        selected,
        Selection {
            polygon: true,
            events: vec![0, 1, 2, 3],
            credential: true
        }
    );
    assert!(parse_fields("polygon", 8).is_err());
    assert!(parse_fields("credential", 8).is_err());
    assert!(parse_fields("events[8]", 8).is_err());
    assert!(parse_fields("events[3..0]", 8).is_err());
    assert!(parse_fields("events[0],secret", 8).is_err());
}

mod common;
use eudr_disclosure::{
    encode_hex, open_bundle, parse_hex, verify_openings, ChildKind, Level, PublicChild,
};

#[test]
fn selected_openings_reject_tampering_and_never_export_private_keys() {
    let (input, path) = common::fixture();
    let selection = parse_fields("polygon,events[0..3],credential", 8).unwrap();
    let bundle = open_bundle(&input, 17, &selection, path).unwrap();
    assert_eq!(bundle.events.len(), 4);
    let verified = verify_openings(&bundle, [21; 32], [22; 32]).unwrap();
    assert_eq!(verified.lot_id, 17);
    assert_eq!(verified.epoch_id, 7);
    let json = serde_json::to_string(&bundle).unwrap();
    for forbidden in [
        "nullifierKey",
        "nullifierPath",
        "batchNonce",
        "signature",
        &hex::encode(input.nullifier_key),
        &hex::encode(input.batch_nonce),
        &hex::encode(input.events[0].signature),
    ] {
        assert!(
            !json.contains(forbidden),
            "private witness material escaped"
        );
    }
    for index in 4..8 {
        assert!(
            !json.contains(&hex::encode(input.events[index].event_bytes)),
            "an unselected event escaped"
        );
    }
    let mut bad = bundle.clone();
    bad.events[0].bytes = encode_hex([0; 86]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.events[0].salt = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.events[0].path[0] = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.events[0].index = 7;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.polygon.as_mut().unwrap().nonce = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.credential.as_mut().unwrap().key_commitment = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.credential.as_mut().unwrap().owner_identity = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.credential.as_mut().unwrap().certificate_id = 999;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.credential.as_mut().unwrap().path[0] = encode_hex([0; 32]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.credential.as_mut().unwrap().event_index = 7;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.lot_id = 18;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    assert!(verify_openings(&bundle, [0; 32], [22; 32]).is_err());
    assert!(open_bundle(&input, 18, &selection, bundle.authentication.clone()).is_err());
}

#[test]
fn ordered_public_parent_path_binds_nested_epochs_and_child_kind() {
    let (input, path) = common::fixture();
    let selected = parse_fields("events[0]", 8).unwrap();
    let mut bundle = open_bundle(&input, 17, &selected, path).unwrap();
    let parent = verify_openings(&bundle, [21; 32], [22; 32])
        .unwrap()
        .root_public_values;
    bundle.authentication.levels.push(Level {
        leaf_v_key_digest: encode_hex([21; 32]),
        aggregate_v_key_digest: encode_hex([22; 32]),
        child_index: 0,
        children: vec![PublicChild {
            kind: ChildKind::Aggregate,
            public_values: encode_hex(parent),
        }],
    });
    assert!(verify_openings(&bundle, [21; 32], [22; 32]).is_ok());
    let mut bad = bundle.clone();
    bad.authentication.levels[1].children[0].kind = ChildKind::Leaf;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.authentication.levels[1].child_index = 1;
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    let mut sibling = parse_hex::<352>(&bad.leaf_public_values).unwrap();
    sibling[31] = 8;
    sibling[32..64].copy_from_slice(&parent[64..96]);
    sibling[64..96].fill(100);
    sibling[96..128].fill(101);
    bad.authentication.levels[1].children.push(PublicChild {
        kind: ChildKind::Leaf,
        public_values: encode_hex(sibling),
    });
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
    let mut bad = bundle.clone();
    bad.authentication.levels[0].children[0].public_values = encode_hex([0; 352]);
    assert!(verify_openings(&bad, [21; 32], [22; 32]).is_err());
}
