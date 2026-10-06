use epcis_mapper::{map_object_event, map_with_catalog, sign_canonical, IdCatalog, MappingContext};
use serde_json::Value;

fn event() -> Value {
    serde_json::from_str(include_str!("../fixtures/object-event.json")).unwrap()
}
fn context() -> Value {
    serde_json::from_str(include_str!("../fixtures/mapping-context.json")).unwrap()
}

#[test]
fn quantity_event_preserves_exact_coordinates_timezone_and_scaled_reading() {
    let mapped = map_object_event(&event(), &context()).unwrap();
    assert_eq!(mapped.latitude_e6, -10_123_456);
    assert_eq!(mapped.longitude_e6, 106_123_456);
    assert_eq!(mapped.timestamp_ms, 1_791_185_400_125);
    assert_eq!(mapped.readings, 825);
    assert_eq!(mapped.certificate_id, 42);
    assert_eq!(mapped.canonical_bytes().len(), 86);
}

#[test]
fn absent_location_signer_certificate_and_sensor_fail_closed() {
    for field in ["readPoint", "sensorElementList"] {
        let mut value = event();
        value.as_object_mut().unwrap().remove(field);
        assert!(map_object_event(&value, &context()).is_err(), "{field}");
    }
    for field in ["actor_public_key", "certificate_id"] {
        let mut value = context();
        value.as_object_mut().unwrap().remove(field);
        assert!(map_object_event(&event(), &value).is_err(), "{field}");
    }
}

#[test]
fn rejects_unsupported_types_multi_lot_uri_time_and_numeric_errors() {
    let changes = [
        ("/type", Value::from("AggregationEvent")),
        ("/type", Value::from("TransformationEvent")),
        ("/eventID", Value::from("relative-identifier")),
        ("/eventTime", Value::from("2026-10-05T14:30:00")),
        ("/eventTime", Value::from("2026-10-05T14:30:00.0001+07:00")),
        ("/eventTimeZoneOffset", Value::from("+08:00")),
        ("/readPoint/id", Value::from("geo:-10.1234567,106")),
        ("/readPoint/id", Value::from("geo:91,106")),
        ("/readPoint/id", Value::from("geo:10,181")),
        (
            "/quantityList/0/epcClass",
            Value::from("urn:epc:class:lgtin:bad"),
        ),
        ("/sensorElementList/0/sensorReport/0/value", Value::from(-1)),
        (
            "/sensorElementList/0/sensorReport/0/value",
            Value::from(42_949_673_u64),
        ),
        (
            "/sensorElementList/0/sensorReport/0/uom",
            Value::from("KEL"),
        ),
    ];
    for (path, change) in changes {
        let mut value = event();
        *value.pointer_mut(path).unwrap() = change;
        assert!(map_object_event(&value, &context()).is_err(), "{path}");
    }
    let mut value = event();
    let lot = value["quantityList"][0].clone();
    value["quantityList"].as_array_mut().unwrap().push(lot);
    assert!(map_object_event(&value, &context()).is_err());
    let mut value = event();
    let report = value["sensorElementList"][0]["sensorReport"][0].clone();
    value["sensorElementList"][0]["sensorReport"]
        .as_array_mut()
        .unwrap()
        .push(report);
    assert!(map_object_event(&value, &context()).is_err());
}

#[test]
fn epc_list_utc_and_boundary_decimals_map_without_rounding() {
    let mut value = event();
    let lot = value["quantityList"][0]["epcClass"].clone();
    value.as_object_mut().unwrap().remove("quantityList");
    value["epcList"] = serde_json::json!([lot]);
    value["eventTime"] = Value::from("2026-10-05T07:30:00.125Z");
    value["eventTimeZoneOffset"] = Value::from("+00:00");
    value["readPoint"]["id"] = Value::from("geo:-90,-180.000000");
    value["sensorElementList"][0]["sensorReport"][0]["value"] =
        serde_json::from_str("42949672.95").unwrap();
    let mapped = map_object_event(&value, &context()).unwrap();
    assert_eq!(mapped.timestamp_ms, 1_791_185_400_125);
    assert_eq!(mapped.latitude_e6, -90_000_000);
    assert_eq!(mapped.longitude_e6, -180_000_000);
    assert_eq!(mapped.readings, u32::MAX);
    value["sensorElementList"][0]["sensorReport"][0]["value"] =
        serde_json::from_str("8.251").unwrap();
    assert!(map_object_event(&value, &context()).is_err());
    value["sensorElementList"][0]["sensorReport"][0]["value"] =
        serde_json::from_str("8.25e0").unwrap();
    assert_eq!(map_object_event(&value, &context()).unwrap().readings, 825);
}

#[test]
fn identifier_catalog_detects_collision_without_partial_mutation() {
    let context: MappingContext = serde_json::from_value(context()).unwrap();
    let mut catalog = IdCatalog::default();
    let mapped = map_with_catalog(&event(), &context, &mut catalog).unwrap();
    assert_eq!(catalog.identifiers.len(), 2);
    assert_eq!(mapped.canonical_hex.len(), 172);
    assert_eq!(mapped.provenance.source_sensor_value, "8.25");
    assert!(mapped.signature_hex.is_none());
    let again = map_with_catalog(&event(), &context, &mut catalog).unwrap();
    assert_eq!(mapped.canonical_hex, again.canonical_hex);
    let key = format!("lot:{}", mapped.provenance.lot_id);
    catalog
        .identifiers
        .insert(key, "urn:epc:class:lgtin:0614141.112345.other".into());
    catalog
        .identifiers
        .remove(&format!("event:{}", mapped.provenance.event_id));
    let before = catalog.identifiers.clone();
    assert!(map_with_catalog(&event(), &context, &mut catalog)
        .unwrap_err()
        .contains("collision"));
    assert_eq!(catalog.identifiers, before);
}

#[test]
fn rejects_unsafe_contexts_both_lot_lists_and_ambiguous_sensor_statistics() {
    let mut value = event();
    value["@context"] = serde_json::json!(["https://ref.gs1.org/standards/epcis/2.0.0/epcis-context.jsonld", {"eventID": "https://example.com/"}]);
    assert!(map_object_event(&value, &context()).is_err());
    value["@context"] = Value::from("https://example.com/context.jsonld");
    assert!(map_object_event(&value, &context()).is_err());
    let mut value = event();
    value["epcList"] = serde_json::json!([]);
    assert!(map_object_event(&value, &context()).is_err());
    let mut value = event();
    value["sensorElementList"][0]["sensorReport"][0]["meanValue"] = Value::from(8);
    assert!(map_object_event(&value, &context()).is_err());
    let mut config = context();
    config["kpi"]["scale"] = Value::from(3);
    assert!(map_object_event(&event(), &config).is_err());
    config["kpi"]["scale"] = Value::from(0);
    assert!(map_object_event(&event(), &config).is_err());
    config = context();
    config["actor_public_key"] = Value::from("00".repeat(32));
    assert!(map_object_event(&event(), &config).is_err());
}

#[test]
fn canonical_signature_uses_only_explicit_matching_private_key() {
    use ed25519_dalek::{Signature, VerifyingKey};
    let context: MappingContext = serde_json::from_value(context()).unwrap();
    let mut source = event();
    source["signature"] = Value::from("untrusted-original-json-signature");
    let mut mapped = map_with_catalog(&source, &context, &mut IdCatalog::default()).unwrap();
    assert!(mapped.signature_hex.is_none());
    assert!(sign_canonical(&mut mapped, &[0x22; 32]).is_err());
    sign_canonical(&mut mapped, &[0x11; 32]).unwrap();
    let signature = Signature::from_bytes(
        &epcis_mapper::decode_hex::<64>(mapped.signature_hex.as_ref().unwrap()).unwrap(),
    );
    let public = VerifyingKey::from_bytes(
        &epcis_mapper::decode_hex::<32>(&context.actor_public_key).unwrap(),
    )
    .unwrap();
    public
        .verify_strict(&mapped.canonical_bytes, &signature)
        .unwrap();
    mapped.canonical_bytes[33] ^= 1;
    assert!(public
        .verify_strict(&mapped.canonical_bytes, &signature)
        .is_err());
}
