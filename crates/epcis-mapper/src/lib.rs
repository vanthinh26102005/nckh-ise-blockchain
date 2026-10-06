//! Host ingestion of a compact GS1 EPCIS 2 ObjectEvent subset. No remote fetch.
use chrono::DateTime;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use eudr_policy::{EpcisEventV1, EPCIS_EVENT_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const EVENT_DOMAIN: &[u8] = b"EUDR:EPCIS:EVENT-ID:V1\0";
const LOT_DOMAIN: &[u8] = b"EUDR:EPCIS:LGTIN:V1\0";
const CONTEXTS: &[&str] = &[
    "https://ref.gs1.org/standards/epcis/2.0.0/epcis-context.jsonld",
    "https://ref.gs1.org/standards/epcis/2.0.1/epcis-context.jsonld",
    "https://ref.gs1.org/standards/epcis/epcis-context.jsonld",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingContext {
    pub epoch_id: u64,
    pub actor_public_key: String,
    pub certificate_id: u64,
    pub role: u8,
    pub kpi: KpiMapping,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KpiMapping {
    pub sensor_type: String,
    pub unit: String,
    /// The canonical u32 stores value * scale; scale must be a power of ten.
    pub scale: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdCatalog {
    /// Keys: event:<decimal u64> or lot:<decimal u64>; values: original URIs.
    pub identifiers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Provenance {
    pub mapping_version: &'static str,
    pub source_event_id: String,
    pub source_lgtin: String,
    pub event_id: u64,
    pub lot_id: u64,
    pub source_event_time: String,
    pub source_geolocation: String,
    pub source_sensor_value: String,
    pub context: MappingContext,
    pub source_event_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MappedEvent {
    pub canonical_hex: String,
    pub canonical_bytes: Vec<u8>,
    pub provenance: Provenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_hex: Option<String>,
}

pub fn map_object_event(event: &Value, context: &Value) -> Result<EpcisEventV1, String> {
    let context: MappingContext = serde_json::from_value(context.clone())
        .map_err(|error| format!("invalid mapping context: {error}"))?;
    map(event, &context)
}

/// Registers both IDs only after all validation and collision checks pass.
pub fn map_with_catalog(
    event: &Value,
    context: &MappingContext,
    catalog: &mut IdCatalog,
) -> Result<MappedEvent, String> {
    let mapped = map(event, context)?;
    let event_uri = text(event, "eventID")?;
    let lot = lot_uri(event)?;
    let entries = [
        (format!("event:{}", mapped.event_id), event_uri),
        (format!("lot:{}", mapped.lot_id), lot),
    ];
    for (key, uri) in &entries {
        if catalog
            .identifiers
            .get(key)
            .is_some_and(|existing| existing != uri)
        {
            return Err(format!("truncated identifier collision at {key}"));
        }
    }
    let bytes = mapped.canonical_bytes();
    let result = MappedEvent {
        canonical_hex: hex(&bytes),
        canonical_bytes: bytes.to_vec(),
        signature_hex: None,
        provenance: Provenance {
            mapping_version: "epcis-object-v1",
            source_event_id: event_uri.into(),
            source_lgtin: lot.into(),
            event_id: mapped.event_id,
            lot_id: mapped.lot_id,
            source_event_time: text(event, "eventTime")?.into(),
            source_geolocation: text(&event["readPoint"], "id")?.into(),
            source_sensor_value: number_text(&sensor_report(event, &context.kpi)?["value"])?,
            context: context.clone(),
            source_event_sha256: hex(&Sha256::digest(
                serde_json::to_vec(event).map_err(|_| "invalid event JSON")?,
            )),
        },
    };
    for (key, uri) in entries {
        catalog.identifiers.insert(key, uri.into());
    }
    Ok(result)
}

/// Signs only canonical bytes; caller supplies a separate private key.
pub fn sign_canonical(mapped: &mut MappedEvent, secret: &[u8; 32]) -> Result<(), String> {
    let bytes: &[u8; EPCIS_EVENT_BYTES] = mapped
        .canonical_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "invalid canonical event length")?;
    let event = eudr_policy::decode_event(bytes).map_err(|_| "invalid canonical event")?;
    let key = SigningKey::from_bytes(secret);
    if key.verifying_key().to_bytes() != event.actor_public_key {
        return Err("private key does not match the mapped actor public key".into());
    }
    mapped.signature_hex = Some(hex(&key.sign(bytes).to_bytes()));
    Ok(())
}

fn map(event: &Value, context: &MappingContext) -> Result<EpcisEventV1, String> {
    if !event.is_object() || text(event, "type")? != "ObjectEvent" {
        return Err(
            "only ObjectEvent is supported; Aggregation/Transformation are not mapped".into(),
        );
    }
    validate_context(&event["@context"], &context.kpi.sensor_type)?;
    if !["ADD", "OBSERVE", "DELETE"].contains(&text(event, "action")?) {
        return Err("invalid ObjectEvent action".into());
    }
    let event_uri = text(event, "eventID")?;
    validate_uri(event_uri)?;
    let lot = lot_uri(event)?;
    let timestamp_ms = timestamp(event)?;
    let (latitude_e6, longitude_e6) = geolocation(text(&event["readPoint"], "id")?)?;
    let actor_public_key = decode_hex::<32>(&context.actor_public_key)
        .map_err(|_| "actor_public_key must be 32-byte hex")?;
    let key =
        VerifyingKey::from_bytes(&actor_public_key).map_err(|_| "invalid Ed25519 public key")?;
    if key.is_weak() {
        return Err("weak Ed25519 public key".into());
    }
    let kpi = &context.kpi;
    if kpi.sensor_type.is_empty()
        || !valid_unit(&kpi.unit)
        || kpi.scale == 0
        || !power_of_ten(kpi.scale)
    {
        return Err("KPI needs a sensor type, CEFACT unit, and positive power-of-ten scale".into());
    }
    let report = sensor_report(event, kpi)?;
    let readings = u32::try_from(scaled_decimal(&number_text(&report["value"])?, kpi.scale)?)
        .map_err(|_| "KPI reading is negative or overflows u32")?;
    Ok(EpcisEventV1 {
        event_id: identifier(EVENT_DOMAIN, event_uri),
        lot_id: identifier(LOT_DOMAIN, lot),
        epoch_id: context.epoch_id,
        timestamp_ms,
        readings,
        latitude_e6,
        longitude_e6,
        certificate_id: context.certificate_id,
        role: context.role,
        actor_public_key,
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("missing or invalid {key}"))
}

fn validate_context(value: &Value, sensor_type: &str) -> Result<(), String> {
    if let Some(uri) = value.as_str() {
        return if CONTEXTS.contains(&uri) {
            Ok(())
        } else {
            Err("unsupported JSON-LD context; remote fetch disabled".into())
        };
    }
    let entries = value
        .as_array()
        .ok_or("a supported GS1 JSON-LD context is required")?;
    if entries.len() == 1 {
        return validate_context(&entries[0], sensor_type);
    }
    if entries.is_empty()
        || !entries[0]
            .as_str()
            .is_some_and(|uri| CONTEXTS.contains(&uri))
    {
        return Err("context array must start with a supported GS1 context".into());
    }
    // Only namespace prefixes may extend the pinned compact context.
    for extension in &entries[1..] {
        for (prefix, namespace) in extension
            .as_object()
            .ok_or("only extension-prefix contexts are supported")?
        {
            if prefix.is_empty()
                || prefix == sensor_type
                || ["ObjectEvent", "ADD", "OBSERVE", "DELETE"].contains(&prefix.as_str())
                || !prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || [
                    "epcis",
                    "cbv",
                    "gs1",
                    "xsd",
                    "type",
                    "id",
                    "eventID",
                    "eventTime",
                    "eventTimeZoneOffset",
                    "action",
                    "epcList",
                    "quantityList",
                    "epcClass",
                    "quantity",
                    "uom",
                    "readPoint",
                    "sensorElementList",
                    "sensorReport",
                    "value",
                ]
                .contains(&prefix.as_str())
            {
                return Err("JSON-LD core term overrides are unsupported".into());
            }
            let namespace = namespace
                .as_str()
                .ok_or("extension namespace must be an absolute URI")?;
            validate_uri(namespace)?;
            if !namespace.ends_with('/') && !namespace.ends_with('#') {
                return Err("extension namespace must end in / or #".into());
            }
        }
    }
    Ok(())
}

fn lot_uri(event: &Value) -> Result<&str, String> {
    let lot = match (event.get("epcList"), event.get("quantityList")) {
        (Some(epcs), None) => epcs
            .as_array()
            .filter(|items| items.len() == 1)
            .ok_or("epcList must contain exactly one LGTIN lot")?[0]
            .as_str()
            .ok_or("epcList identifier must be a LGTIN URI")?,
        (None, Some(quantities)) => {
            let quantity = &quantities
                .as_array()
                .filter(|items| items.len() == 1)
                .ok_or("quantityList must contain exactly one LGTIN lot")?[0];
            if decimal(&number_text(&quantity["quantity"])?)?.0 <= 0 {
                return Err("quantity must be a positive decimal".into());
            }
            if quantity
                .get("uom")
                .is_some_and(|unit| !unit.as_str().is_some_and(valid_unit))
            {
                return Err("invalid quantity unit".into());
            }
            text(quantity, "epcClass")?
        }
        _ => return Err("provide exactly one of epcList or quantityList".into()),
    };
    validate_lgtin(lot)?;
    Ok(lot)
}

fn validate_lgtin(uri: &str) -> Result<(), String> {
    validate_uri(uri)?;
    let parts: Vec<_> = uri
        .strip_prefix("urn:epc:class:lgtin:")
        .ok_or("only LGTIN lot URIs are supported")?
        .split('.')
        .collect();
    if parts.len() != 3
        || !(6..=12).contains(&parts[0].len())
        || parts[0].len() + parts[1].len() != 13
        || !parts[0].bytes().all(|b| b.is_ascii_digit())
        || !parts[1].bytes().all(|b| b.is_ascii_digit())
        || parts[2].is_empty()
    {
        return Err("invalid LGTIN company prefix, item reference, or lot".into());
    }
    let batch = parts[2].as_bytes();
    let mut index = 0;
    while index < batch.len() {
        if batch[index] == b'%' {
            let a = batch[index + 1];
            let b = batch[index + 2];
            if a.is_ascii_lowercase() || b.is_ascii_lowercase() {
                return Err("LGTIN escapes must be uppercase".into());
            }
            let decoded =
                (a as char).to_digit(16).unwrap() * 16 + (b as char).to_digit(16).unwrap();
            if !(33..=126).contains(&decoded)
                || (decoded as u8).is_ascii_alphanumeric()
                || matches!(decoded as u8, b'-' | b'_')
            {
                return Err("invalid or noncanonical LGTIN percent escape".into());
            }
            index += 3;
        } else {
            if !batch[index].is_ascii_alphanumeric() && !matches!(batch[index], b'-' | b'_') {
                return Err("LGTIN lot requires percent encoding for reserved characters".into());
            }
            index += 1;
        }
    }
    Ok(())
}

fn validate_uri(uri: &str) -> Result<(), String> {
    let (scheme, body) = uri
        .split_once(':')
        .ok_or("identifier must be an absolute URI")?;
    if scheme.is_empty()
        || !scheme.as_bytes()[0].is_ascii_alphabetic()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        || body.is_empty()
        || !uri.is_ascii()
    {
        return Err("invalid absolute URI".into());
    }
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err("invalid URI percent escape".into());
            }
            index += 3;
        } else {
            if !bytes[index].is_ascii_alphanumeric()
                && !b"-._~:/?#[]@!$&'()*+,;=".contains(&bytes[index])
            {
                return Err("invalid URI character".into());
            }
            index += 1;
        }
    }
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        let authority = body
            .strip_prefix("//")
            .ok_or("HTTP URI needs an authority")?
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("");
        if authority.is_empty() || authority.contains('@') || authority.starts_with(':') {
            return Err("invalid HTTP authority".into());
        }
    }
    Ok(())
}

fn timestamp(event: &Value) -> Result<u64, String> {
    let raw = text(event, "eventTime")?;
    let zone = text(event, "eventTimeZoneOffset")?;
    let bytes = raw.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || ![
            &bytes[0..4],
            &bytes[5..7],
            &bytes[8..10],
            &bytes[11..13],
            &bytes[14..16],
            &bytes[17..19],
        ]
        .iter()
        .all(|part| part.iter().all(u8::is_ascii_digit))
    {
        return Err("eventTime must be strict RFC3339 with an explicit timezone".into());
    }
    let zone_start = if raw.ends_with('Z') {
        raw.len() - 1
    } else {
        raw.len()
            .checked_sub(6)
            .ok_or("eventTime needs a timezone")?
    };
    let fraction = raw
        .get(19..zone_start)
        .ok_or("invalid eventTime timezone")?;
    if !fraction.is_empty()
        && (!fraction.starts_with('.')
            || !(2..=4).contains(&fraction.len())
            || !fraction[1..].bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("eventTime precision must fit exact milliseconds".into());
    }
    let parsed = DateTime::parse_from_rfc3339(raw).map_err(|_| "invalid RFC3339 eventTime")?;
    if parsed.timestamp_subsec_nanos() >= 1_000_000_000
        || parsed.timestamp_subsec_nanos() % 1_000_000 != 0
    {
        return Err("leap seconds or submillisecond time unsupported".into());
    }
    let zone_bytes = zone.as_bytes();
    if zone_bytes.len() != 6
        || !matches!(zone_bytes[0], b'+' | b'-')
        || zone_bytes[3] != b':'
        || ![zone_bytes[1], zone_bytes[2], zone_bytes[4], zone_bytes[5]]
            .iter()
            .all(u8::is_ascii_digit)
        || zone == "-00:00"
    {
        return Err("invalid eventTimeZoneOffset".into());
    }
    let hours: i32 = zone[1..3].parse().map_err(|_| "invalid timezone hours")?;
    let minutes: i32 = zone[4..6].parse().map_err(|_| "invalid timezone minutes")?;
    if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
        return Err("timezone offset outside ±14:00".into());
    }
    let offset = (hours * 3600 + minutes * 60) * if zone_bytes[0] == b'-' { -1 } else { 1 };
    if parsed.offset().local_minus_utc() != offset || raw.ends_with("-00:00") {
        return Err("eventTime timezone does not match eventTimeZoneOffset".into());
    }
    u64::try_from(parsed.timestamp_millis()).map_err(|_| "eventTime precedes Unix epoch".into())
}

fn geolocation(uri: &str) -> Result<(i32, i32), String> {
    let (lat, lon) = uri
        .strip_prefix("geo:")
        .ok_or("readPoint.id must be geo:latitude,longitude")?
        .split_once(',')
        .ok_or("geo URI needs two coordinates")?;
    if lat.contains(['e', 'E', '+']) || lon.contains(['e', 'E', '+']) {
        return Err("geo URI requires plain decimals".into());
    }
    let lat = i32::try_from(scaled_decimal(lat, 1_000_000)?).map_err(|_| "latitude overflow")?;
    let lon = i32::try_from(scaled_decimal(lon, 1_000_000)?).map_err(|_| "longitude overflow")?;
    eudr_policy::PointE6::new(lat, lon)
        .validate()
        .map_err(|_| "coordinates outside WGS84 bounds")?;
    Ok((lat, lon))
}

fn sensor_report<'a>(event: &'a Value, kpi: &KpiMapping) -> Result<&'a Value, String> {
    let elements = event["sensorElementList"]
        .as_array()
        .filter(|elements| !elements.is_empty())
        .ok_or("sensorElementList required")?;
    let mut selected = None;
    for element in elements {
        for report in element["sensorReport"]
            .as_array()
            .filter(|reports| !reports.is_empty())
            .ok_or("sensorReport must be nonempty")?
        {
            if text(report, "type")? == kpi.sensor_type {
                if selected.is_some() {
                    return Err("ambiguous KPI sensor reports".into());
                }
                if text(report, "uom")? != kpi.unit {
                    return Err("KPI sensor unit mismatch".into());
                }
                if [
                    "component",
                    "minValue",
                    "maxValue",
                    "meanValue",
                    "sDev",
                    "percRank",
                    "percValue",
                    "booleanValue",
                    "stringValue",
                    "hexBinaryValue",
                    "uriValue",
                ]
                .iter()
                .any(|key| report.get(*key).is_some())
                {
                    return Err("KPI requires one scalar value without competing statistics".into());
                }
                selected = Some(report);
            }
        }
    }
    selected.ok_or_else(|| "missing KPI sensor report matching sensor_type".into())
}

fn valid_unit(unit: &str) -> bool {
    (2..=3).contains(&unit.len())
        && unit
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
fn number_text(value: &Value) -> Result<String, String> {
    value
        .as_number()
        .map(ToString::to_string)
        .ok_or_else(|| "numeric decimal required".into())
}
fn power_of_ten(mut scale: u32) -> bool {
    while scale > 1 && scale % 10 == 0 {
        scale /= 10;
    }
    scale == 1
}

/// Parse coefficient and exponent without floating-point conversion.
fn decimal(raw: &str) -> Result<(i128, i32), String> {
    if raw.is_empty() || raw.len() > 256 {
        return Err("invalid or oversized decimal".into());
    }
    let (mantissa, exponent) = match raw.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| "decimal exponent overflow")?,
        ),
        None => (raw, 0),
    };
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(value) => (true, value),
        None => (false, mantissa),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return Err("empty decimal fraction".into()),
        None => (mantissa, ""),
    };
    if whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err("invalid decimal digits".into());
    }
    let digits = format!("{whole}{fraction}");
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Ok((0, 0));
    }
    let trimmed = significant.trim_end_matches('0');
    let coefficient = trimmed
        .parse::<i128>()
        .map_err(|_| "decimal coefficient overflow")?;
    let exponent = exponent
        .checked_sub(fraction.len() as i32)
        .and_then(|value| value.checked_add((significant.len() - trimmed.len()) as i32))
        .ok_or("decimal exponent overflow")?;
    Ok((if negative { -coefficient } else { coefficient }, exponent))
}

fn scaled_decimal(raw: &str, scale: u32) -> Result<i128, String> {
    let (coefficient, exponent) = decimal(raw)?;
    let exponent = exponent
        .checked_add(scale.ilog10() as i32)
        .ok_or("decimal scaling overflow")?;
    if coefficient == 0 {
        return Ok(0);
    }
    if exponent < 0 {
        return Err("decimal is not exact at configured scale".into());
    }
    coefficient
        .checked_mul(
            10_i128
                .checked_pow(exponent as u32)
                .ok_or("decimal scaling overflow")?,
        )
        .ok_or_else(|| "decimal scaling overflow".into())
}

fn identifier(domain: &[u8], uri: &str) -> u64 {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(uri.as_bytes());
    u64::from_be_bytes(
        digest.finalize()[..8]
            .try_into()
            .expect("SHA256 prefix width"),
    )
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub fn decode_hex<const N: usize>(raw: &str) -> Result<[u8; N], String> {
    if raw.len() != N * 2 || !raw.is_ascii() {
        return Err("invalid hex width".into());
    }
    let mut output = [0; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid hex digit")?;
    }
    Ok(output)
}
