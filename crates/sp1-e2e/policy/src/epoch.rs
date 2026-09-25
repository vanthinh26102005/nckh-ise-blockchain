//! The public contract between SP1 policy leaves and one epoch aggregate.
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const LEAF_PUBLIC_BYTES: usize = 352;
pub const EPOCH_PUBLIC_BYTES: usize = 288;
pub const MAX_FAN_IN: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EpochChild {
    Leaf([u8; LEAF_PUBLIC_BYTES]),
    Aggregate([u8; EPOCH_PUBLIC_BYTES]),
}

impl EpochChild {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Leaf(value) => value,
            Self::Aggregate(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochInput {
    pub leaf_vk_digest: [u8; 32],
    pub aggregate_vk_digest: [u8; 32],
    pub children: Vec<EpochChild>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpochPublicValues {
    pub epoch_id: u64,
    pub old_nullifier_root: [u8; 32],
    pub new_nullifier_root: [u8; 32],
    pub epoch_commitment: [u8; 32],
    pub lot_count: u64,
    pub event_count: u64,
    pub valid: bool,
    pub leaf_vk_digest: [u8; 32],
    pub aggregate_vk_digest: [u8; 32],
}

impl EpochInput {
    pub fn encode(&self) -> Result<Vec<u8>, &'static str> {
        if !(1..=MAX_FAN_IN).contains(&self.children.len()) {
            return Err("epoch fan-in must be 1..=8");
        }
        let mut out = Vec::with_capacity(66 + self.children.len() * (1 + LEAF_PUBLIC_BYTES));
        out.push(1); // wire version
        out.extend_from_slice(&self.leaf_vk_digest);
        out.extend_from_slice(&self.aggregate_vk_digest);
        out.push(self.children.len() as u8);
        for child in &self.children {
            out.push(match child {
                EpochChild::Leaf(_) => 0,
                EpochChild::Aggregate(_) => 1,
            });
            out.extend_from_slice(child.bytes());
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 66 || bytes[0] != 1 || !(1..=MAX_FAN_IN).contains(&(bytes[65] as usize)) {
            return Err("invalid epoch input header");
        }
        let mut cursor = 66;
        let mut children = Vec::with_capacity(bytes[65] as usize);
        for _ in 0..bytes[65] {
            let kind = *bytes.get(cursor).ok_or("truncated epoch child")?;
            cursor += 1;
            let width = match kind {
                0 => LEAF_PUBLIC_BYTES,
                1 => EPOCH_PUBLIC_BYTES,
                _ => return Err("invalid epoch child kind"),
            };
            let data = bytes
                .get(cursor..cursor + width)
                .ok_or("truncated epoch child")?;
            children.push(if kind == 0 {
                EpochChild::Leaf(data.try_into().unwrap())
            } else {
                EpochChild::Aggregate(data.try_into().unwrap())
            });
            cursor += width;
        }
        if cursor != bytes.len() {
            return Err("trailing epoch input bytes");
        }
        Ok(Self {
            leaf_vk_digest: bytes[1..33].try_into().unwrap(),
            aggregate_vk_digest: bytes[33..65].try_into().unwrap(),
            children,
        })
    }

    pub fn vk_words(digest: &[u8; 32]) -> [u32; 8] {
        std::array::from_fn(|i| u32::from_be_bytes(digest[4 * i..4 * i + 4].try_into().unwrap()))
    }
}

impl EpochPublicValues {
    /// Solidity ABI: (uint64,bytes32,bytes32,bytes32,uint64,uint64,bool,bytes32,bytes32).
    pub fn abi_encode(self) -> [u8; EPOCH_PUBLIC_BYTES] {
        let mut out = [0; EPOCH_PUBLIC_BYTES];
        out[24..32].copy_from_slice(&self.epoch_id.to_be_bytes());
        out[32..64].copy_from_slice(&self.old_nullifier_root);
        out[64..96].copy_from_slice(&self.new_nullifier_root);
        out[96..128].copy_from_slice(&self.epoch_commitment);
        out[152..160].copy_from_slice(&self.lot_count.to_be_bytes());
        out[184..192].copy_from_slice(&self.event_count.to_be_bytes());
        out[223] = u8::from(self.valid);
        out[224..256].copy_from_slice(&self.leaf_vk_digest);
        out[256..288].copy_from_slice(&self.aggregate_vk_digest);
        out
    }
}

fn word_u64(bytes: &[u8], index: usize) -> Result<u64, &'static str> {
    let word = &bytes[index * 32..(index + 1) * 32];
    if word[..24] != [0; 24] {
        return Err("noncanonical ABI integer");
    }
    Ok(u64::from_be_bytes(word[24..].try_into().unwrap()))
}

/// Evaluate only authenticated leaf *public values*. The guest also verifies every
/// compressed child proof against their SHA-256 digest and `leaf_vk_digest`.
pub fn evaluate(input: &EpochInput) -> Result<EpochPublicValues, &'static str> {
    if !(1..=MAX_FAN_IN).contains(&input.children.len()) {
        return Err("epoch fan-in must be 1..=8");
    }
    let mut commitment = Sha256::new();
    commitment.update(b"EUDR:E3:EPOCH:V1");
    commitment.update(input.leaf_vk_digest);
    commitment.update(input.aggregate_vk_digest);
    commitment.update([input.children.len() as u8]);
    let mut epoch_id = None;
    let mut old_root = [0; 32];
    let mut current_root = [0; 32];
    let mut event_count = 0_u64;
    let mut lot_count = 0_u64;
    let mut seen_batches = HashSet::new();

    for (index, child) in input.children.iter().enumerate() {
        let value = child.bytes();
        let id = word_u64(value, 0)?;
        let (lots, events) = match child {
            EpochChild::Leaf(_) => {
                let events = word_u64(value, 9)?;
                if word_u64(value, 7)? != super::REQUIRED_ROLE as u64
                    || word_u64(value, 8)? != super::MAX_ALLOWED_READING as u64
                    || word_u64(value, 10)? != 1
                    || !(super::MIN_EVENTS as u64..=super::MAX_EVENTS as u64).contains(&events)
                {
                    return Err("invalid policy leaf public values");
                }
                (1, events)
            }
            EpochChild::Aggregate(_) => {
                let lots = word_u64(value, 4)?;
                let events = word_u64(value, 5)?;
                let minimum = lots.checked_mul(8).ok_or("aggregate lot count overflow")?;
                let maximum = lots.checked_mul(64).ok_or("aggregate lot count overflow")?;
                if word_u64(value, 6)? != 1
                    || value[224..256] != input.leaf_vk_digest
                    || value[256..288] != input.aggregate_vk_digest
                    || lots == 0
                    || events < minimum
                    || events > maximum
                {
                    return Err("invalid aggregate child public values");
                }
                (lots, events)
            }
        };
        if epoch_id.is_some_and(|expected| expected != id) {
            return Err("leaf belongs to another epoch");
        }
        epoch_id = Some(id);
        if index == 0 {
            old_root.copy_from_slice(&value[32..64]);
            current_root = old_root;
        }
        if value[32..64] != current_root {
            return Err("broken nullifier-root chain");
        }
        current_root.copy_from_slice(&value[64..96]);
        if current_root == value[32..64] {
            return Err("nullifier root did not advance");
        }
        if !seen_batches.insert(&value[96..128]) {
            return Err("duplicate child commitment");
        }
        lot_count = lot_count.checked_add(lots).ok_or("lot count overflow")?;
        event_count = event_count
            .checked_add(events)
            .ok_or("event count overflow")?;
        commitment.update([matches!(child, EpochChild::Aggregate(_)) as u8]);
        commitment.update(&value[96..128]);
        commitment.update(&value[32..96]);
        commitment.update(lots.to_be_bytes());
        commitment.update(events.to_be_bytes());
    }
    Ok(EpochPublicValues {
        epoch_id: epoch_id.unwrap(),
        old_nullifier_root: old_root,
        new_nullifier_root: current_root,
        epoch_commitment: commitment.finalize().into(),
        lot_count,
        event_count,
        valid: true,
        leaf_vk_digest: input.leaf_vk_digest,
        aggregate_vk_digest: input.aggregate_vk_digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(epoch: u64, old: u8, new: u8, digest: u8) -> [u8; LEAF_PUBLIC_BYTES] {
        let mut value = [0; LEAF_PUBLIC_BYTES];
        value[24..32].copy_from_slice(&epoch.to_be_bytes());
        value[32..64].fill(old);
        value[64..96].fill(new);
        value[96..128].fill(digest);
        value[255] = super::super::REQUIRED_ROLE;
        value[284..288].copy_from_slice(&super::super::MAX_ALLOWED_READING.to_be_bytes());
        value[319] = 8;
        value[351] = 1;
        value
    }

    #[test]
    fn two_leaves_bind_order_epoch_and_root_chain() {
        let input = EpochInput {
            leaf_vk_digest: [7; 32],
            aggregate_vk_digest: [8; 32],
            children: vec![
                EpochChild::Leaf(leaf(9, 0, 1, 1)),
                EpochChild::Leaf(leaf(9, 1, 2, 2)),
            ],
        };
        assert_eq!(evaluate(&input).unwrap().event_count, 16);
        assert_eq!(EpochInput::decode(&input.encode().unwrap()).unwrap(), input);
        let mut bad = input.clone();
        bad.children.swap(0, 1);
        assert!(evaluate(&bad).is_err());
        bad = input.clone();
        if let EpochChild::Leaf(value) = &mut bad.children[1] {
            value[31] = 10;
        }
        assert!(evaluate(&bad).is_err());
        bad = input.clone();
        let first_batch = bad.children[0].bytes()[96..128].to_vec();
        if let EpochChild::Leaf(value) = &mut bad.children[1] {
            value[96..128].copy_from_slice(&first_batch);
        }
        assert!(evaluate(&bad).is_err());
        let first = evaluate(&input).unwrap().abi_encode();
        let nested = EpochInput {
            children: vec![EpochChild::Aggregate(first)],
            ..input
        };
        assert_eq!(evaluate(&nested).unwrap().lot_count, 2);
        let mut wrong_key = nested.clone();
        wrong_key.leaf_vk_digest[0] ^= 1;
        assert_eq!(
            evaluate(&wrong_key),
            Err("invalid aggregate child public values")
        );
        let mut wrong_validity = nested.clone();
        if let EpochChild::Aggregate(value) = &mut wrong_validity.children[0] {
            value[223] = 0;
        }
        assert_eq!(
            evaluate(&wrong_validity),
            Err("invalid aggregate child public values")
        );
        let too_many = EpochInput {
            children: (0..=MAX_FAN_IN)
                .map(|i| EpochChild::Leaf(leaf(9, i as u8, i as u8 + 1, i as u8 + 1)))
                .collect(),
            ..nested
        };
        assert_eq!(evaluate(&too_many), Err("epoch fan-in must be 1..=8"));
    }
}
