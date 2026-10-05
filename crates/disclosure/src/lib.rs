//! Selective policy-v2 openings. This crate does not generate or verify SP1 proofs.
pub mod rpc;

use eudr_policy::{
    self as policy,
    epoch::{self, EpochChild, EpochInput},
    PointE6, PolicyInput, EPCIS_EVENT_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub polygon: bool,
    pub events: Vec<usize>,
    pub credential: bool,
}

pub fn parse_fields(fields: &str, event_count: usize) -> Result<Selection, String> {
    let mut selection = Selection {
        polygon: false,
        events: vec![],
        credential: false,
    };
    let mut events = BTreeSet::new();
    for field in fields.split(',') {
        match field {
            "polygon" => selection.polygon = true,
            "credential" => selection.credential = true,
            _ => {
                let range = field
                    .strip_prefix("events[")
                    .and_then(|s| s.strip_suffix(']'))
                    .ok_or("unknown disclosure field")?;
                let parse = |s: &str| {
                    s.parse::<usize>()
                        .map_err(|_| "invalid event index".to_string())
                };
                let (first, last) = if let Some((first, last)) = range.split_once("..") {
                    (parse(first)?, parse(last)?)
                } else {
                    let index = parse(range)?;
                    (index, index)
                };
                if first > last || last >= event_count {
                    return Err("event selection is outside the lot".into());
                }
                events.extend(first..=last);
            }
        }
    }
    selection.events = events.into_iter().collect();
    if selection.events.is_empty() {
        return Err("include an event to authenticate lot".into());
    }
    Ok(selection)
}

pub fn preset(name: &str) -> Result<&'static str, String> {
    match name {
        "regulator" => Ok("polygon,events[0..3],credential"),
        "importer" => Ok("events[0..3]"),
        "certifier" => Ok("events[0],credential"),
        _ => Err("preset must be regulator, importer or certifier".into()),
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthenticationPath {
    pub protocol_version: u8,
    pub transaction_hash: String,
    pub levels: Vec<Level>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Level {
    pub leaf_v_key_digest: String,
    pub aggregate_v_key_digest: String,
    pub child_index: usize,
    pub children: Vec<PublicChild>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChildKind {
    Leaf,
    Aggregate,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicChild {
    pub kind: ChildKind,
    pub public_values: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bundle {
    pub protocol_version: u8,
    pub lot_id: u64,
    pub leaf_public_values: String,
    pub authentication: AuthenticationPath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon: Option<PolygonOpening>,
    pub events: Vec<EventOpening>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialOpening>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolygonOpening {
    pub nonce: String,
    pub vertices: Vec<[i32; 2]>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventOpening {
    pub index: usize,
    pub bytes: String,
    pub salt: String,
    pub path: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialOpening {
    pub event_index: usize,
    pub credential_index: u16,
    pub owner_identity: String,
    pub key_commitment: String,
    pub lot_id: u64,
    pub certificate_id: u64,
    pub actor_public_key: String,
    pub role: u8,
    pub path: Vec<String>,
}

pub fn encode_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(bytes))
}
pub fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    if value.len() != 2 + N * 2 || !value.starts_with("0x") {
        return Err(format!("expected {N}-byte hex value"));
    }
    hex::decode(&value[2..])
        .map_err(|_| "invalid hex value".to_string())?
        .try_into()
        .map_err(|_| "invalid hex length".into())
}
fn hashes(values: &[String]) -> Result<Vec<[u8; 32]>, String> {
    values.iter().map(|value| parse_hex(value)).collect()
}
fn word_u64(value: &[u8; 352], index: usize) -> Result<u64, String> {
    let word = &value[index * 32..(index + 1) * 32];
    if word[..24] != [0; 24] {
        return Err("noncanonical ABI integer".into());
    }
    Ok(u64::from_be_bytes(word[24..].try_into().unwrap()))
}
fn hash_word(value: &[u8; 352], index: usize) -> [u8; 32] {
    value[index * 32..(index + 1) * 32].try_into().unwrap()
}

pub struct VerifiedBundle {
    pub epoch_id: u64,
    pub lot_id: u64,
    pub root_public_values: [u8; 352],
}
fn verify_authentication(
    leaf: [u8; 352],
    authentication: &AuthenticationPath,
    leaf_vk: [u8; 32],
    aggregate_vk: [u8; 32],
) -> Result<[u8; 352], String> {
    if authentication.protocol_version != 2
        || authentication.levels.is_empty()
        || authentication.levels.len() > 64
    {
        return Err("invalid policy-v2 authentication depth".into());
    }
    parse_hex::<32>(&authentication.transaction_hash)?;
    // The policy evaluator checks canonical leaf ABI and every ordered parent.
    epoch::evaluate(&EpochInput {
        leaf_vk_digest: leaf_vk,
        aggregate_vk_digest: aggregate_vk,
        children: vec![EpochChild::Leaf(leaf)],
    })
    .map_err(str::to_string)?;
    let mut current = leaf;
    let mut kind = ChildKind::Leaf;
    for level in &authentication.levels {
        if parse_hex::<32>(&level.leaf_v_key_digest)? != leaf_vk
            || parse_hex::<32>(&level.aggregate_v_key_digest)? != aggregate_vk
        {
            return Err(
                "authentication verification keys do not match expected configuration".into(),
            );
        }
        let selected = level
            .children
            .get(level.child_index)
            .ok_or("authentication child index is out of range")?;
        if selected.kind != kind || parse_hex::<352>(&selected.public_values)? != current {
            return Err("authentication path does not include the target child".into());
        }
        let children = level
            .children
            .iter()
            .map(|child| {
                Ok(match child.kind {
                    ChildKind::Leaf => EpochChild::Leaf(parse_hex(&child.public_values)?),
                    ChildKind::Aggregate => EpochChild::Aggregate(parse_hex(&child.public_values)?),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        current = epoch::evaluate(&EpochInput {
            leaf_vk_digest: leaf_vk,
            aggregate_vk_digest: aggregate_vk,
            children,
        })
        .map_err(str::to_string)?
        .abi_encode();
        kind = ChildKind::Aggregate;
    }
    Ok(current)
}

pub fn open_bundle(
    input: &PolicyInput,
    lot: u64,
    selection: &Selection,
    authentication: AuthenticationPath,
) -> Result<Bundle, String> {
    if lot != input.lot_id {
        return Err("requested lot does not match the private witness".into());
    }
    if selection.events.is_empty()
        || selection
            .events
            .iter()
            .any(|index| *index >= input.events.len())
    {
        return Err("include a valid selected event to authenticate lot".into());
    }
    let leaf = policy::evaluate(input)
        .map_err(|_| "private witness fails policy-v2 evaluation".to_string())?
        .abi_encode();
    let tree = policy::event_tree(&input.events, input.batch_nonce);
    let events = selection
        .events
        .iter()
        .map(|index| {
            let mut slot = *index;
            let mut path = vec![];
            for level in tree.iter().take(tree.len() - 1) {
                path.push(encode_hex(level[slot ^ 1]));
                slot >>= 1;
            }
            EventOpening {
                index: *index,
                bytes: encode_hex(input.events[*index].event_bytes),
                salt: encode_hex(policy::event_salt(input.batch_nonce, *index)),
                path,
            }
        })
        .collect();
    let polygon = selection.polygon.then(|| PolygonOpening {
        nonce: encode_hex(input.polygon_nonce),
        vertices: input
            .polygon
            .iter()
            .map(|point| [point.latitude_e6, point.longitude_e6])
            .collect(),
    });
    let credential = if selection.credential {
        let index = selection.events[0];
        let witness = &input.events[index];
        let event = policy::decode_event(&witness.event_bytes)
            .map_err(|_| "invalid credential event".to_string())?;
        Some(CredentialOpening {
            event_index: index,
            credential_index: witness.credential_index,
            owner_identity: encode_hex(input.owner_identity),
            key_commitment: encode_hex(policy::nullifier_key_commitment(input.nullifier_key)),
            lot_id: event.lot_id,
            certificate_id: event.certificate_id,
            actor_public_key: encode_hex(event.actor_public_key),
            role: event.role,
            path: witness.certificate_path.iter().map(encode_hex).collect(),
        })
    } else {
        None
    };
    let bundle = Bundle {
        protocol_version: 2,
        lot_id: lot,
        leaf_public_values: encode_hex(leaf),
        authentication,
        polygon,
        events,
        credential,
    };
    let first = bundle
        .authentication
        .levels
        .first()
        .ok_or("authentication path is empty")?;
    verify_openings(
        &bundle,
        parse_hex(&first.leaf_v_key_digest)?,
        parse_hex(&first.aggregate_v_key_digest)?,
    )?;
    Ok(bundle)
}

pub fn verify_openings(
    bundle: &Bundle,
    leaf_vk: [u8; 32],
    aggregate_vk: [u8; 32],
) -> Result<VerifiedBundle, String> {
    if bundle.protocol_version != 2
        || bundle.events.is_empty()
        || bundle.events.len() > policy::MAX_EVENTS
    {
        return Err("policy-v2 view requires selected events to authenticate lot".into());
    }
    let leaf = parse_hex::<352>(&bundle.leaf_public_values)?;
    let root_public_values =
        verify_authentication(leaf, &bundle.authentication, leaf_vk, aggregate_vk)?;
    let epoch_id = word_u64(&leaf, 0)?;
    let count = word_u64(&leaf, 9)? as usize;
    let mut seen = BTreeSet::new();
    for opening in &bundle.events {
        if !seen.insert(opening.index) {
            return Err("duplicate disclosed event index".into());
        }
        let bytes = parse_hex::<EPCIS_EVENT_BYTES>(&opening.bytes)?;
        if !policy::verify_event_opening(
            count,
            opening.index,
            parse_hex(&opening.salt)?,
            &bytes,
            &hashes(&opening.path)?,
            hash_word(&leaf, 3),
        ) {
            return Err("event opening does not match the target batch".into());
        }
        let event = policy::decode_event(&bytes)
            .map_err(|_| "disclosed event is not canonical".to_string())?;
        if event.lot_id != bundle.lot_id || event.epoch_id != epoch_id {
            return Err("event belongs to another lot or epoch".into());
        }
    }
    if let Some(polygon) = &bundle.polygon {
        let vertices = polygon
            .vertices
            .iter()
            .map(|point| PointE6::new(point[0], point[1]))
            .collect::<Vec<_>>();
        if !(3..=policy::MAX_POLYGON_VERTICES).contains(&vertices.len())
            || policy::polygon_commitment(&vertices, parse_hex(&polygon.nonce)?)
                != hash_word(&leaf, 5)
        {
            return Err("polygon opening does not match its commitment".into());
        }
    }
    if let Some(credential) = &bundle.credential {
        let opening = bundle
            .events
            .iter()
            .find(|event| event.index == credential.event_index)
            .ok_or("credential must match a selected event opening")?;
        let bytes = parse_hex::<EPCIS_EVENT_BYTES>(&opening.bytes)?;
        let event =
            policy::decode_event(&bytes).map_err(|_| "invalid credential event".to_string())?;
        if credential.lot_id != event.lot_id
            || credential.certificate_id != event.certificate_id
            || credential.role != event.role
            || parse_hex::<32>(&credential.actor_public_key)? != event.actor_public_key
        {
            return Err("credential does not match the disclosed event".into());
        }
        let path = hashes(&credential.path)?
            .try_into()
            .map_err(|_| "credential path must have 16 siblings".to_string())?;
        if policy::credential_root(
            credential.credential_index as usize,
            &event,
            parse_hex(&credential.owner_identity)?,
            parse_hex(&credential.key_commitment)?,
            &path,
        ) != hash_word(&leaf, 4)
        {
            return Err("credential opening does not match its registry root".into());
        }
    }
    Ok(VerifiedBundle {
        epoch_id,
        lot_id: bundle.lot_id,
        root_public_values,
    })
}

pub mod cli {
    use serde::de::DeserializeOwned;
    use std::{
        collections::HashMap,
        fs::{self, OpenOptions},
        io::Write,
        path::Path,
    };
    pub fn args(allowed: &[&str]) -> Result<HashMap<String, String>, String> {
        let mut values = HashMap::new();
        let mut args = std::env::args().skip(1);
        while let Some(name) = args.next() {
            if name == "--help" {
                values.insert(name, String::new());
                continue;
            }
            if !allowed.contains(&name.as_str()) {
                return Err("unknown command option".into());
            }
            let value = args
                .next()
                .filter(|value| !value.starts_with("--"))
                .ok_or("command option requires a value")?;
            if values.insert(name, value).is_some() {
                return Err("duplicate command option".into());
            }
        }
        Ok(values)
    }
    pub fn required<'a>(args: &'a HashMap<String, String>, name: &str) -> Result<&'a str, String> {
        args.get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("{name} is required"))
    }
    pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
        let metadata = fs::metadata(path.as_ref()).map_err(|_| "input file is unavailable")?;
        if metadata.len() > 1_000_000 {
            return Err("input file is too large".into());
        }
        fs::read(path).map_err(|_| "input file could not be read".into())
    }
    pub fn json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
        serde_json::from_slice(bytes)
            .map_err(|_| "input JSON does not match the documented schema".into())
    }
    pub fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "output must be a new writable file")?;
        file.write_all(bytes)
            .map_err(|_| "output could not be written".into())
    }
}
