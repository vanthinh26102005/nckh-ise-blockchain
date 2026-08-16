use e1_bench::epcis::{EpcisEventV1, PointE6, SimplePolygon, EPCIS_EVENT_V1_BYTES};
use e1_bench::{synthetic::synthetic_lot, Profile};

fn event() -> EpcisEventV1 {
    EpcisEventV1 {
        event_id: 0x0102_0304_0506_0708,
        lot_id: 0x1112_1314_1516_1718,
        epoch_id: 0x2122_2324_2526_2728,
        timestamp_ms: 1_725_000_123_456,
        readings: 900,
        latitude_e6: -10_778_901,
        longitude_e6: 106_700_123,
        certificate_id: 0x3132_3334_3536_3738,
        role: 7,
        actor_public_key: [0xa5; 32],
    }
}

#[test]
fn canonical_event_encoding_is_fixed_and_round_trips() {
    let event = event();
    let bytes = event.canonical_bytes();
    assert_eq!(bytes.len(), EPCIS_EVENT_V1_BYTES);
    assert_eq!(&bytes[..9], &[1, 1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&bytes[9..17], &event.lot_id.to_be_bytes());
    assert_eq!(&bytes[17..25], &event.epoch_id.to_be_bytes());
    assert_eq!(&bytes[25..33], &event.timestamp_ms.to_be_bytes());
    assert_eq!(&bytes[33..37], &event.readings.to_be_bytes());
    assert_eq!(&bytes[37..41], &event.latitude_e6.to_be_bytes());
    assert_eq!(&bytes[41..45], &event.longitude_e6.to_be_bytes());
    assert_eq!(&bytes[45..53], &event.certificate_id.to_be_bytes());
    assert_eq!(bytes[53], event.role);
    assert_eq!(&bytes[54..], &event.actor_public_key);
    assert_eq!(EpcisEventV1::decode_canonical(&bytes).unwrap(), event);
}

#[test]
fn decoder_rejects_noncanonical_payloads() {
    let event = event();
    let mut wrong_version = event.canonical_bytes();
    wrong_version[0] = 2;
    assert!(EpcisEventV1::decode_canonical(&wrong_version).is_err());
    assert!(EpcisEventV1::decode_canonical(&wrong_version[..85]).is_err());

    let mut invalid_latitude = event.canonical_bytes();
    invalid_latitude[37..41].copy_from_slice(&90_000_001i32.to_be_bytes());
    assert!(EpcisEventV1::decode_canonical(&invalid_latitude).is_err());
}

#[test]
fn polygon_is_simple_and_strictly_classifies_concave_points() {
    let polygon = SimplePolygon::new(vec![
        PointE6::new(0, 0),
        PointE6::new(10, 0),
        PointE6::new(10, 10),
        PointE6::new(5, 5),
        PointE6::new(0, 10),
    ])
    .unwrap();
    assert!(polygon.contains_strict(PointE6::new(2, 2)));
    assert!(!polygon.contains_strict(PointE6::new(5, 8)));
    assert!(!polygon.contains_strict(PointE6::new(5, 5)));
    assert!(!polygon.contains_strict(PointE6::new(5, 0)));
}

#[test]
fn polygon_rejects_self_intersection_and_invalid_size() {
    assert!(SimplePolygon::new(vec![PointE6::new(0, 0), PointE6::new(1, 1)]).is_err());
    assert!(SimplePolygon::new(vec![
        PointE6::new(0, 0),
        PointE6::new(10, 10),
        PointE6::new(0, 10),
        PointE6::new(10, 0),
    ])
    .is_err());
}

#[test]
fn synthetic_lot_uses_canonical_epcis_events() {
    let lot = synthetic_lot(9, 8, Profile::CoffeeSmall);
    assert_eq!(lot.events.len(), 8);
    for (index, event) in lot.events.iter().enumerate() {
        assert_eq!(event.lot_id, lot.lot_id);
        assert_eq!(event.epoch_id, lot.epoch);
        assert_eq!(event.readings, lot.readings[index] as u32);
        assert_eq!(
            EpcisEventV1::decode_canonical(&event.canonical_bytes()).unwrap(),
            *event
        );
    }
}
