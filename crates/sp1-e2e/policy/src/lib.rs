use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use hmac::{Hmac, Mac};
use std::collections::HashMap;

pub mod epoch;
pub mod registry;

pub const EPCIS_EVENT_BYTES: usize = 86;
pub const ED25519_SIGNATURE_BYTES: usize = 64;
pub const MAX_POLYGON_VERTICES: usize = 32;
pub const MIN_EVENTS: usize = 8;
pub const MAX_EVENTS: usize = 64;
pub const CERTIFICATE_TREE_DEPTH: usize = 16;
pub const NULLIFIER_TREE_DEPTH: usize = 256;
pub const REQUIRED_ROLE: u8 = 4;
pub const MAX_ALLOWED_READING: u32 = 900;

pub const INPUT_VERSION: u8 = 2;
const EVENT_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    InvalidInput,
    UnsupportedEventVersion,
    InvalidCoordinate,
    InvalidPolygon,
    C1Geofence,
    C2CredentialRoot,
    C3ThresholdOrTime,
    C4SignatureOrRole,
    C5Nullifier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointE6 {
    pub latitude_e6: i32,
    pub longitude_e6: i32,
}

impl PointE6 {
    pub const fn new(latitude_e6: i32, longitude_e6: i32) -> Self {
        Self {
            latitude_e6,
            longitude_e6,
        }
    }

    pub fn validate(self) -> Result<(), PolicyError> {
        if !(-90_000_000..=90_000_000).contains(&self.latitude_e6)
            || !(-180_000_000..=180_000_000).contains(&self.longitude_e6)
        {
            return Err(PolicyError::InvalidCoordinate);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpcisEventV1 {
    pub event_id: u64,
    pub lot_id: u64,
    pub epoch_id: u64,
    pub timestamp_ms: u64,
    pub readings: u32,
    pub latitude_e6: i32,
    pub longitude_e6: i32,
    pub certificate_id: u64,
    pub role: u8,
    pub actor_public_key: [u8; 32],
}

impl EpcisEventV1 {
    pub fn point(self) -> PointE6 {
        PointE6 {
            latitude_e6: self.latitude_e6,
            longitude_e6: self.longitude_e6,
        }
    }

    pub fn canonical_bytes(self) -> [u8; EPCIS_EVENT_BYTES] {
        let mut output = [0_u8; EPCIS_EVENT_BYTES];
        output[0] = EVENT_VERSION;
        output[1..9].copy_from_slice(&self.event_id.to_be_bytes());
        output[9..17].copy_from_slice(&self.lot_id.to_be_bytes());
        output[17..25].copy_from_slice(&self.epoch_id.to_be_bytes());
        output[25..33].copy_from_slice(&self.timestamp_ms.to_be_bytes());
        output[33..37].copy_from_slice(&self.readings.to_be_bytes());
        output[37..41].copy_from_slice(&self.latitude_e6.to_be_bytes());
        output[41..45].copy_from_slice(&self.longitude_e6.to_be_bytes());
        output[45..53].copy_from_slice(&self.certificate_id.to_be_bytes());
        output[53] = self.role;
        output[54..86].copy_from_slice(&self.actor_public_key);
        output
    }
}

pub fn decode_event(bytes: &[u8; EPCIS_EVENT_BYTES]) -> Result<EpcisEventV1, PolicyError> {
    if bytes[0] != EVENT_VERSION {
        return Err(PolicyError::UnsupportedEventVersion);
    }
    let event = EpcisEventV1 {
        event_id: u64::from_be_bytes(bytes[1..9].try_into().expect("fixed event width")),
        lot_id: u64::from_be_bytes(bytes[9..17].try_into().expect("fixed event width")),
        epoch_id: u64::from_be_bytes(bytes[17..25].try_into().expect("fixed event width")),
        timestamp_ms: u64::from_be_bytes(bytes[25..33].try_into().expect("fixed event width")),
        readings: u32::from_be_bytes(bytes[33..37].try_into().expect("fixed event width")),
        latitude_e6: i32::from_be_bytes(bytes[37..41].try_into().expect("fixed event width")),
        longitude_e6: i32::from_be_bytes(bytes[41..45].try_into().expect("fixed event width")),
        certificate_id: u64::from_be_bytes(bytes[45..53].try_into().expect("fixed event width")),
        role: bytes[53],
        actor_public_key: bytes[54..86].try_into().expect("fixed event width"),
    };
    event.point().validate()?;
    Ok(event)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventWitness {
    pub event_bytes: [u8; EPCIS_EVENT_BYTES],
    pub signature: [u8; ED25519_SIGNATURE_BYTES],
    pub credential_index: u16,
    pub certificate_path: [[u8; 32]; CERTIFICATE_TREE_DEPTH],
}

#[derive(Clone, Eq, PartialEq)]
pub struct PolicyInput {
    pub epoch_id: u64,
    pub lot_id: u64,
    pub polygon: Vec<PointE6>,
    pub certificate_root: [u8; 32],
    pub old_nullifier_root: [u8; 32],
    pub nullifier_key: [u8; 32],
    pub owner_identity: [u8; 32],
    pub polygon_nonce: [u8; 32],
    pub batch_nonce: [u8; 32],
    pub nullifier_path: [[u8; 32]; NULLIFIER_TREE_DEPTH],
    pub events: Vec<EventWitness>,
}

impl std::fmt::Debug for PolicyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyInput").field("epoch_id",&self.epoch_id).field("lot_id",&self.lot_id).field("event_count",&self.events.len()).finish_non_exhaustive()
    }
}
impl PolicyInput {
    pub fn encode(&self) -> Result<Vec<u8>, PolicyError> {
        validate_polygon(&self.polygon)?;
        if !(MIN_EVENTS..=MAX_EVENTS).contains(&self.events.len()) {
            return Err(PolicyError::InvalidInput);
        }

        let mut bytes = Vec::with_capacity(
            1 + 8 + 8 + 1 + self.polygon.len() * 8 + 96 + 32 * 32 + self.events.len() * 342,
        );
        bytes.push(INPUT_VERSION);
        bytes.extend_from_slice(&self.epoch_id.to_be_bytes());
        bytes.extend_from_slice(&self.lot_id.to_be_bytes());
        bytes.push(self.polygon.len() as u8);
        for point in &self.polygon {
            bytes.extend_from_slice(&point.latitude_e6.to_be_bytes());
            bytes.extend_from_slice(&point.longitude_e6.to_be_bytes());
        }
        bytes.extend_from_slice(&self.certificate_root);
        bytes.extend_from_slice(&self.old_nullifier_root);
        bytes.extend_from_slice(&self.nullifier_key);
        bytes.extend_from_slice(&self.owner_identity);
        bytes.extend_from_slice(&self.polygon_nonce);
        bytes.extend_from_slice(&self.batch_nonce);
        for sibling in &self.nullifier_path {
            bytes.extend_from_slice(sibling);
        }
        bytes.push(self.events.len() as u8);
        for witness in &self.events {
            bytes.extend_from_slice(&witness.event_bytes);
            bytes.extend_from_slice(&witness.signature);
            bytes.extend_from_slice(&witness.credential_index.to_be_bytes());
            for sibling in &witness.certificate_path {
                bytes.extend_from_slice(sibling);
            }
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PolicyError> {
        let mut reader = ByteReader::new(bytes);
        if reader.byte()? != INPUT_VERSION {
            return Err(PolicyError::InvalidInput);
        }
        let epoch_id = reader.u64()?;
        let lot_id = reader.u64()?;
        let polygon_len = reader.byte()? as usize;
        if !(3..=MAX_POLYGON_VERTICES).contains(&polygon_len) {
            return Err(PolicyError::InvalidPolygon);
        }
        let mut polygon = Vec::with_capacity(polygon_len);
        for _ in 0..polygon_len {
            polygon.push(PointE6 {
                latitude_e6: reader.i32()?,
                longitude_e6: reader.i32()?,
            });
        }
        validate_polygon(&polygon)?;
        let certificate_root = reader.array::<32>()?;
        let old_nullifier_root = reader.array::<32>()?;
        let nullifier_key = reader.array::<32>()?;
        let owner_identity = reader.array::<32>()?;
        let polygon_nonce = reader.array::<32>()?;
        let batch_nonce = reader.array::<32>()?;
        let mut nullifier_path = [[0_u8; 32]; NULLIFIER_TREE_DEPTH];
        for sibling in &mut nullifier_path {
            *sibling = reader.array()?;
        }
        let event_count = reader.byte()? as usize;
        if !(MIN_EVENTS..=MAX_EVENTS).contains(&event_count) {
            return Err(PolicyError::InvalidInput);
        }
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            let event_bytes = reader.array()?;
            let signature = reader.array()?;
            let credential_index = u16::from_be_bytes(reader.array()?);
            let mut certificate_path = [[0_u8; 32]; CERTIFICATE_TREE_DEPTH];
            for sibling in &mut certificate_path {
                *sibling = reader.array()?;
            }
            events.push(EventWitness {
                event_bytes,
                signature,
                credential_index,
                certificate_path,
            });
        }
        if !reader.finished() {
            return Err(PolicyError::InvalidInput);
        }
        Ok(Self {
            epoch_id,
            lot_id,
            polygon,
            certificate_root,
            old_nullifier_root,
            nullifier_key,
            owner_identity,
            polygon_nonce,
            batch_nonce,
            nullifier_path,
            events,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPublicValues {
    pub epoch_id: u64,
    pub old_nullifier_root: [u8; 32],
    pub new_nullifier_root: [u8; 32],
    pub event_batch_digest: [u8; 32],
    pub certificate_root: [u8; 32],
    pub polygon_commitment: [u8; 32],
    pub nullifier: [u8; 32],
    pub role: u8,
    pub threshold: u32,
    pub event_count: u8,
    pub valid: bool,
}

impl PolicyPublicValues {
    pub fn abi_decode(bytes:&[u8]) -> Result<Self,PolicyError> {
        if bytes.len()!=352 {return Err(PolicyError::InvalidInput);}
        let word=|i|crate::epoch::word_u64(bytes,i).map_err(|_|PolicyError::InvalidInput);
        let role=u8::try_from(word(7)?).map_err(|_|PolicyError::InvalidInput)?;
        let threshold=u32::try_from(word(8)?).map_err(|_|PolicyError::InvalidInput)?;
        let event_count=u8::try_from(word(9)?).map_err(|_|PolicyError::InvalidInput)?;
        let valid=word(10)?;if valid>1 {return Err(PolicyError::InvalidInput);}
        Ok(Self {epoch_id:word(0)?,old_nullifier_root:bytes[32..64].try_into().unwrap(),new_nullifier_root:bytes[64..96].try_into().unwrap(),
            event_batch_digest:bytes[96..128].try_into().unwrap(),certificate_root:bytes[128..160].try_into().unwrap(),polygon_commitment:bytes[160..192].try_into().unwrap(),
            nullifier:bytes[192..224].try_into().unwrap(),role,threshold,event_count,valid:valid==1})
    }

    /// Solidity ABI encoding for `(uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint8,uint32,uint8,bool)`.
    pub fn abi_encode(self) -> [u8; 352] {
        let mut output = [0_u8; 352];
        output[24..32].copy_from_slice(&self.epoch_id.to_be_bytes());
        output[32..64].copy_from_slice(&self.old_nullifier_root);
        output[64..96].copy_from_slice(&self.new_nullifier_root);
        output[96..128].copy_from_slice(&self.event_batch_digest);
        output[128..160].copy_from_slice(&self.certificate_root);
        output[160..192].copy_from_slice(&self.polygon_commitment);
        output[192..224].copy_from_slice(&self.nullifier);
        output[255] = self.role;
        output[284..288].copy_from_slice(&self.threshold.to_be_bytes());
        output[319] = self.event_count;
        output[351] = u8::from(self.valid);
        output
    }
}

pub fn evaluate(input: &PolicyInput) -> Result<PolicyPublicValues, PolicyError> {
    validate_polygon(&input.polygon)?;
    if !(MIN_EVENTS..=MAX_EVENTS).contains(&input.events.len()) {
        return Err(PolicyError::InvalidInput);
    }

    let mut previous_timestamp = None;

    for witness in &input.events {
        let event = decode_event(&witness.event_bytes)?;
        if event.lot_id != input.lot_id
            || event.epoch_id != input.epoch_id
            || event.role != REQUIRED_ROLE
            || event.readings > MAX_ALLOWED_READING
            || previous_timestamp.is_some_and(|timestamp| event.timestamp_ms <= timestamp)
        {
            return Err(PolicyError::C3ThresholdOrTime);
        }
        previous_timestamp = Some(event.timestamp_ms);
        if !is_strictly_outside(&input.polygon, event.point()) {
            return Err(PolicyError::C1Geofence);
        }
        let public_key = VerifyingKey::from_bytes(&event.actor_public_key)
            .map_err(|_| PolicyError::C4SignatureOrRole)?;
        let signature = Signature::from_bytes(&witness.signature);
        public_key
            .verify_strict(&witness.event_bytes, &signature)
            .map_err(|_| PolicyError::C4SignatureOrRole)?;
        if credential_root(witness.credential_index as usize, &event, input.owner_identity, nullifier_key_commitment(input.nullifier_key), &witness.certificate_path) != input.certificate_root {
            return Err(PolicyError::C2CredentialRoot);
        }
    }

    let nullifier = nullifier_hash(input.lot_id, input.nullifier_key);
    let index = nullifier_index(input.lot_id, input.nullifier_key);
    let old_root = sparse_root(index, empty_leaf(), &input.nullifier_path);
    if old_root != input.old_nullifier_root {
        return Err(PolicyError::C5Nullifier);
    }
    let new_root = sparse_root(index, used_leaf(), &input.nullifier_path);

    Ok(PolicyPublicValues {
        epoch_id: input.epoch_id,
        old_nullifier_root: input.old_nullifier_root,
        new_nullifier_root: new_root,
        event_batch_digest: event_batch_commitment(&input.events, input.batch_nonce),
        certificate_root: input.certificate_root,
        polygon_commitment: polygon_commitment(&input.polygon, input.polygon_nonce),
        nullifier,
        role: REQUIRED_ROLE,
        threshold: MAX_ALLOWED_READING,
        event_count: input.events.len() as u8,
        valid: true,
    })
}

pub fn polygon_commitment(polygon: &[PointE6], nonce: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"EUDR:E2E:C1:POLYGON:V2");
    hasher.update(nonce);
    hasher.update([polygon.len() as u8]);
    for point in polygon {
        hasher.update(point.latitude_e6.to_be_bytes());
        hasher.update(point.longitude_e6.to_be_bytes());
    }
    hasher.finalize().into()
}

pub fn certificate_leaf(index: usize, event: &EpcisEventV1, owner: [u8; 32], key_commitment: [u8; 32]) -> [u8; 32] {
    hash_parts(&[
        b"EUDR:E2E:C2:CREDENTIAL:V2",
        &event.lot_id.to_be_bytes(),
        &owner,
        &key_commitment,
        &(index as u64).to_be_bytes(),
        &event.certificate_id.to_be_bytes(),
        &event.actor_public_key,
        &[event.role],
    ])
}

pub fn certificate_parent(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    hash_parts(&[b"EUDR:E2E:C2:NODE:V2", &left, &right])
}

pub fn certificate_padding_leaf(index: usize) -> [u8; 32] {
    hash_parts(&[b"EUDR:E2E:C2:PADDING:V2", &(index as u64).to_be_bytes()])
}

pub fn nullifier_default_root() -> [u8; 32] {
    let mut current = empty_leaf();
    for _ in 0..NULLIFIER_TREE_DEPTH {
        current = nullifier_parent(current, current);
    }
    current
}

pub fn nullifier_default_path() -> [[u8; 32]; NULLIFIER_TREE_DEPTH] {
    let mut path = [[0_u8; 32]; NULLIFIER_TREE_DEPTH];
    let mut current = empty_leaf();
    for sibling in &mut path {
        *sibling = current;
        current = nullifier_parent(current, current);
    }
    path
}

pub fn nullifier_index(lot_id: u64, nullifier_key: [u8; 32]) -> [u8; 32] {
    nullifier_hash(lot_id, nullifier_key)
}

/// Full digest index. Walk least significant bit first, digest in big-endian order.
#[derive(Clone, Debug)]
pub struct SparseNullifierMap {
    nodes: HashMap<(usize, [u8; 32]), [u8; 32]>,
    defaults: [[u8; 32]; NULLIFIER_TREE_DEPTH + 1],
}
impl Default for SparseNullifierMap {
    fn default() -> Self { Self { nodes: HashMap::new(), defaults: nullifier_empty_hashes() } }
}
impl SparseNullifierMap {
    pub fn root(&self) -> [u8; 32] { self.value(NULLIFIER_TREE_DEPTH, [0; 32]) }
    pub fn empty_path(&self, mut index: [u8; 32]) -> Result<[[u8; 32]; NULLIFIER_TREE_DEPTH], PolicyError> {
        if self.value(0, index) != empty_leaf() { return Err(PolicyError::C5Nullifier); }
        let mut path = [[0; 32]; NULLIFIER_TREE_DEPTH];
        for (depth, sibling) in path.iter_mut().enumerate() {
            let mut adjacent = index; adjacent[31] ^= 1;
            *sibling = self.value(depth, adjacent);
            shift_index(&mut index);
        }
        Ok(path)
    }
    pub fn insert(&mut self, mut index: [u8; 32]) -> Result<(), PolicyError> {
        let path = self.empty_path(index)?;
        let mut current = used_leaf();
        self.nodes.insert((0, index), current);
        for (depth, sibling) in path.iter().enumerate() {
            current = if index[31] & 1 == 0 { nullifier_parent(current, *sibling) }
                else { nullifier_parent(*sibling, current) };
            shift_index(&mut index);
            self.nodes.insert((depth + 1, index), current);
        }
        Ok(())
    }
    fn value(&self, depth: usize, index: [u8; 32]) -> [u8; 32] {
        self.nodes.get(&(depth, index)).copied().unwrap_or(self.defaults[depth])
    }
}
fn shift_index(index: &mut [u8; 32]) {
    let mut carry = 0;
    for byte in index { let next = (*byte & 1) << 7; *byte = (*byte >> 1) | carry; carry = next; }
}

fn nullifier_empty_hashes() -> [[u8; 32]; NULLIFIER_TREE_DEPTH + 1] {
    let mut hashes = [[0_u8; 32]; NULLIFIER_TREE_DEPTH + 1];
    hashes[0] = empty_leaf();
    for depth in 0..NULLIFIER_TREE_DEPTH {
        hashes[depth + 1] = nullifier_parent(hashes[depth], hashes[depth]);
    }
    hashes
}

pub fn credential_root(
    mut index: usize,
    event: &EpcisEventV1,
    owner: [u8; 32],
    key_commitment: [u8; 32],
    path: &[[u8; 32]; CERTIFICATE_TREE_DEPTH],
) -> [u8; 32] {
    let mut current = certificate_leaf(index, event, owner, key_commitment);
    for sibling in path {
        current = if index & 1 == 0 {
            certificate_parent(current, *sibling)
        } else {
            certificate_parent(*sibling, current)
        };
        index >>= 1;
    }
    current
}

pub fn sparse_root(
    mut index: [u8; 32],
    mut current: [u8; 32],
    path: &[[u8; 32]; NULLIFIER_TREE_DEPTH],
) -> [u8; 32] {
    for sibling in path {
        current = if index[31] & 1 == 0 {
            nullifier_parent(current, *sibling)
        } else {
            nullifier_parent(*sibling, current)
        };
        shift_index(&mut index);
    }
    current
}

fn empty_leaf() -> [u8; 32] {
    hash_parts(&[b"EUDR:E2E:C5:LEAF:V2", &[0]])
}

fn used_leaf() -> [u8; 32] {
    hash_parts(&[b"EUDR:E2E:C5:LEAF:V2", &[1]])
}

pub fn nullifier_key_commitment(key: [u8; 32]) -> [u8; 32] {
    hash_parts(&[b"EUDR:NULLIFIER-KEY:V2", &key])
}
fn hmac(key: [u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("32-byte HMAC key");
    for part in parts { mac.update(part); }
    mac.finalize().into_bytes().into()
}
pub fn nullifier_hash(lot_id: u64, key: [u8; 32]) -> [u8; 32] {
    let secret = hmac(key, &[b"EUDR:LOT-SECRET:V2", &lot_id.to_be_bytes()]);
    hash_parts(&[b"EUDR:E2E:NULLIFIER:V2", &lot_id.to_be_bytes(), &secret])
}
pub fn event_salt(nonce: [u8; 32], index: usize) -> [u8; 32] {
    hmac(nonce, &[b"EUDR:EVENT-SALT:V2", &(index as u64).to_be_bytes()])
}
pub fn event_leaf(index: usize, salt: [u8; 32], bytes: &[u8; EPCIS_EVENT_BYTES]) -> [u8; 32] {
    hash_parts(&[b"EUDR:EVENT:V2", &(index as u64).to_be_bytes(), &salt, bytes])
}
pub fn event_parent(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    hash_parts(&[b"EUDR:EVENT-NODE:V2", &left, &right])
}
pub fn batch_commitment(count: usize, root: [u8; 32]) -> [u8; 32] {
    hash_parts(&[b"EUDR:EVENT-BATCH:V2", &(count as u64).to_be_bytes(), &root])
}
pub fn event_tree(events: &[EventWitness], nonce: [u8; 32]) -> Vec<Vec<[u8; 32]>> {
    let width = events.len().max(1).next_power_of_two();
    let leaves: Vec<[u8; 32]> = (0..width).map(|i| if i < events.len() {
        event_leaf(i, event_salt(nonce, i), &events[i].event_bytes)
    } else { hash_parts(&[b"EUDR:EVENT-PADDING:V2", &(i as u64).to_be_bytes()]) }).collect();
    let mut tree = vec![leaves];
    while tree.last().unwrap().len() > 1 {
        let next = tree.last().unwrap().chunks_exact(2).map(|x| event_parent(x[0], x[1])).collect();
        tree.push(next);
    }
    tree
}
pub fn event_batch_commitment(events: &[EventWitness], nonce: [u8; 32]) -> [u8; 32] {
    batch_commitment(events.len(), event_tree(events, nonce).last().unwrap()[0])
}
pub fn verify_event_opening(count: usize, index: usize, salt: [u8; 32], bytes: &[u8; EPCIS_EVENT_BYTES], path: &[[u8; 32]], commitment: [u8; 32]) -> bool {
    if !(MIN_EVENTS..=MAX_EVENTS).contains(&count) || index >= count || path.len() != count.next_power_of_two().ilog2() as usize { return false; }
    let mut node = event_leaf(index, salt, bytes); let mut slot = index;
    for sibling in path { node = if slot & 1 == 0 { event_parent(node, *sibling) } else { event_parent(*sibling, node) }; slot >>= 1; }
    batch_commitment(count, node) == commitment
}

fn nullifier_parent(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    hash_parts(&[b"EUDR:E2E:C5:NODE:V2", &left, &right])
}

fn hash_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn validate_polygon(vertices: &[PointE6]) -> Result<(), PolicyError> {
    if !(3..=MAX_POLYGON_VERTICES).contains(&vertices.len()) {
        return Err(PolicyError::InvalidPolygon);
    }
    for point in vertices { point.validate()?; }
    let area: i128=vertices.iter().enumerate().map(|(i,p)| {let q=vertices[(i+1)%vertices.len()];p.longitude_e6 as i128*q.latitude_e6 as i128-q.longitude_e6 as i128*p.latitude_e6 as i128}).sum();
    if area==0 { return Err(PolicyError::InvalidPolygon); }
    for index in 0..vertices.len() {
        if vertices[index] == vertices[(index + 1) % vertices.len()] {
            return Err(PolicyError::InvalidPolygon);
        }
    }
    for first in 0..vertices.len() {
        for second in first + 1..vertices.len() {
            if adjacent(first, second, vertices.len()) {
                continue;
            }
            if segments_intersect(
                vertices[first],
                vertices[(first + 1) % vertices.len()],
                vertices[second],
                vertices[(second + 1) % vertices.len()],
            ) {
                return Err(PolicyError::InvalidPolygon);
            }
        }
    }
    Ok(())
}

fn is_strictly_outside(polygon: &[PointE6], point: PointE6) -> bool {
    if polygon
        .iter()
        .enumerate()
        .any(|(index, start)| point_on_segment(*start, polygon[(index + 1) % polygon.len()], point))
    {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        if (start.latitude_e6 > point.latitude_e6) != (end.latitude_e6 > point.latitude_e6) {
            let side = orientation(start, end, point);
            let crosses_right = if end.latitude_e6 > start.latitude_e6 {
                side > 0
            } else {
                side < 0
            };
            if crosses_right {
                inside = !inside;
            }
        }
    }
    !inside
}

fn adjacent(first: usize, second: usize, len: usize) -> bool {
    first + 1 == second || (first == 0 && second + 1 == len)
}

fn orientation(start: PointE6, end: PointE6, point: PointE6) -> i128 {
    (end.longitude_e6 as i128 - start.longitude_e6 as i128)
        * (point.latitude_e6 as i128 - start.latitude_e6 as i128)
        - (end.latitude_e6 as i128 - start.latitude_e6 as i128)
            * (point.longitude_e6 as i128 - start.longitude_e6 as i128)
}

fn point_on_segment(start: PointE6, end: PointE6, point: PointE6) -> bool {
    orientation(start, end, point) == 0
        && point.longitude_e6 >= start.longitude_e6.min(end.longitude_e6)
        && point.longitude_e6 <= start.longitude_e6.max(end.longitude_e6)
        && point.latitude_e6 >= start.latitude_e6.min(end.latitude_e6)
        && point.latitude_e6 <= start.latitude_e6.max(end.latitude_e6)
}

fn segments_intersect(a: PointE6, b: PointE6, c: PointE6, d: PointE6) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    (ab_c == 0 && point_on_segment(a, b, c))
        || (ab_d == 0 && point_on_segment(a, b, d))
        || (cd_a == 0 && point_on_segment(c, d, a))
        || (cd_b == 0 && point_on_segment(c, d, b))
        || ((ab_c > 0) != (ab_d > 0) && (cd_a > 0) != (cd_b > 0))
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, PolicyError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, PolicyError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, PolicyError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PolicyError> {
        self.take(N)?
            .try_into()
            .map_err(|_| PolicyError::InvalidInput)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], PolicyError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(PolicyError::InvalidInput)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(PolicyError::InvalidInput)?;
        self.offset = end;
        Ok(slice)
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_event_rejects_an_invalid_version() {
        let mut bytes = [0_u8; EPCIS_EVENT_BYTES];
        bytes[0] = 2;
        assert_eq!(
            decode_event(&bytes),
            Err(PolicyError::UnsupportedEventVersion)
        );
    }

    #[test]
    fn geofence_rejects_a_boundary_point() {
        let polygon = [
            PointE6 {
                latitude_e6: 0,
                longitude_e6: 0,
            },
            PointE6 {
                latitude_e6: 0,
                longitude_e6: 10,
            },
            PointE6 {
                latitude_e6: 10,
                longitude_e6: 0,
            },
        ];
        assert!(!is_strictly_outside(
            &polygon,
            PointE6 {
                latitude_e6: 0,
                longitude_e6: 5
            }
        ));
    }

    #[test]
    fn geofence_classifies_concave_polygon_points_without_floats() {
        let polygon = [
            PointE6::new(0, 0),
            PointE6::new(10, 0),
            PointE6::new(10, 10),
            PointE6::new(5, 5),
            PointE6::new(0, 10),
        ];
        assert!(validate_polygon(&polygon).is_ok());
        assert!(is_strictly_outside(&polygon, PointE6::new(5, 8)));
        assert!(!is_strictly_outside(&polygon, PointE6::new(2, 2)));
        assert!(!is_strictly_outside(&polygon, PointE6::new(5, 5)));
    }

    #[test]
    fn geofence_rejects_a_self_intersecting_polygon() {
        let bow_tie = [
            PointE6::new(0, 0),
            PointE6::new(10, 10),
            PointE6::new(0, 10),
            PointE6::new(10, 0),
        ];
        assert_eq!(validate_polygon(&bow_tie), Err(PolicyError::InvalidPolygon));
    }

    #[test]
    fn default_nullifier_root_is_deterministic() {
        assert_eq!(nullifier_default_root(), nullifier_default_root());
    }

    #[test]
    fn sparse_nullifier_map_chains_three_private_witnesses() {
        let indices = [
            nullifier_index(1, [1; 32]),
            nullifier_index(2, [2; 32]),
            nullifier_index(3, [3; 32]),
        ];
        let mut map = SparseNullifierMap::default();
        assert_eq!(map.root(), nullifier_default_root());

        for index in indices {
            let old_root = map.root();
            let path = map.empty_path(index).unwrap();
            assert_eq!(sparse_root(index, empty_leaf(), &path), old_root);
            let expected_new_root = sparse_root(index, used_leaf(), &path);
            map.insert(index).unwrap();
            assert_eq!(map.root(), expected_new_root);
        }

        assert_eq!(map.empty_path(indices[1]), Err(PolicyError::C5Nullifier));
    }
    #[test]
    fn full_digest_indices_do_not_alias_on_the_first_word() {
        let a = [7_u8; 32];
        let mut b = a;
        b[31] ^= 1;
        let mut map = SparseNullifierMap::default();
        map.insert(a).unwrap();
        assert!(map.empty_path(b).is_ok());
        map.insert(b).unwrap();
        assert_eq!(map.empty_path(a), Err(PolicyError::C5Nullifier));
    }

}
