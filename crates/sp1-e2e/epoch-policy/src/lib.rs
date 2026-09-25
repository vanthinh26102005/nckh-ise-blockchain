use eudr_policy::{evaluate_with_nullifier_id, PolicyInput, PolicyPublicValues};
use sha2::{Digest, Sha256};

pub const MAX_FAN_IN: usize = 8;
pub const LEAF_PUBLIC_BYTES: usize = 149;
pub const AGGREGATE_PUBLIC_BYTES: usize = 224;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochError {
    InvalidInput,
    InvalidLeaf,
    InvalidOrder,
    RootMismatch,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeafInput {
    pub shipment_id: u64,
    pub leaf_index: u8,
    pub leaf_count: u8,
    pub policy: PolicyInput,
}

impl LeafInput {
    pub fn encode(&self) -> Result<Vec<u8>, EpochError> {
        let policy = self.policy.encode().map_err(|_| EpochError::InvalidInput)?;
        let mut out = Vec::with_capacity(11 + policy.len());
        out.push(1);
        out.extend_from_slice(&self.shipment_id.to_be_bytes());
        out.push(self.leaf_index);
        out.push(self.leaf_count);
        out.extend_from_slice(&policy);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EpochError> {
        if bytes.len() < 12 || bytes[0] != 1 {
            return Err(EpochError::InvalidInput);
        }
        Ok(Self {
            shipment_id: u64::from_be_bytes(bytes[1..9].try_into().unwrap()),
            leaf_index: bytes[9],
            leaf_count: bytes[10],
            policy: PolicyInput::decode(&bytes[11..]).map_err(|_| EpochError::InvalidInput)?,
        })
    }

    pub fn evaluate(&self) -> Result<LeafPublicValues, EpochError> {
        if self.shipment_id == 0
            || self.policy.lot_id != self.shipment_id
            || self.leaf_count == 0
            || self.leaf_count as usize > MAX_FAN_IN
            || self.leaf_index >= self.leaf_count
        {
            return Err(EpochError::InvalidLeaf);
        }
        let nullifier_id =
            leaf_nullifier_id(self.policy.epoch_id, self.shipment_id, self.leaf_index);
        let p = evaluate_with_nullifier_id(&self.policy, nullifier_id)
            .map_err(|_| EpochError::InvalidLeaf)?;
        Ok(LeafPublicValues::from_policy(self, &p))
    }
}

/// Domain-separated identifier. The full 32-byte nullifier still includes the
/// private secret in the underlying C5 policy.
pub fn leaf_nullifier_id(epoch_id: u64, shipment_id: u64, leaf_index: u8) -> u64 {
    let mut h = Sha256::new();
    h.update(b"EUDR:E3:LEAF-NULLIFIER-ID:V1");
    h.update(epoch_id.to_be_bytes());
    h.update(shipment_id.to_be_bytes());
    h.update([leaf_index]);
    let digest = h.finalize();
    u64::from_be_bytes(digest[..8].try_into().unwrap())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeafPublicValues {
    pub epoch_id: u64,
    pub shipment_id: u64,
    pub leaf_index: u8,
    pub leaf_count: u8,
    pub old_nullifier_root: [u8; 32],
    pub new_nullifier_root: [u8; 32],
    pub event_batch_digest: [u8; 32],
    pub lot_commitment: [u8; 32],
    pub event_count: u16,
    pub valid: bool,
}

impl LeafPublicValues {
    fn from_policy(input: &LeafInput, p: &PolicyPublicValues) -> Self {
        let mut h = Sha256::new();
        h.update(b"EUDR:E3:LOT-LEAF:V1");
        h.update(p.epoch_id.to_be_bytes());
        h.update(input.shipment_id.to_be_bytes());
        h.update([input.leaf_index, input.leaf_count]);
        h.update(p.old_nullifier_root);
        h.update(p.new_nullifier_root);
        h.update(p.event_batch_digest);
        h.update(p.certificate_root);
        h.update(p.polygon_commitment);
        h.update(p.nullifier);
        h.update(p.event_count.to_be_bytes());
        Self {
            epoch_id: p.epoch_id,
            shipment_id: input.shipment_id,
            leaf_index: input.leaf_index,
            leaf_count: input.leaf_count,
            old_nullifier_root: p.old_nullifier_root,
            new_nullifier_root: p.new_nullifier_root,
            event_batch_digest: p.event_batch_digest,
            lot_commitment: h.finalize().into(),
            event_count: u16::from(p.event_count),
            valid: p.valid,
        }
    }

    pub fn encode(self) -> [u8; LEAF_PUBLIC_BYTES] {
        let mut out = [0u8; LEAF_PUBLIC_BYTES];
        out[..8].copy_from_slice(&self.epoch_id.to_be_bytes());
        out[8..16].copy_from_slice(&self.shipment_id.to_be_bytes());
        out[16] = self.leaf_index;
        out[17] = self.leaf_count;
        out[18..50].copy_from_slice(&self.old_nullifier_root);
        out[50..82].copy_from_slice(&self.new_nullifier_root);
        out[82..114].copy_from_slice(&self.event_batch_digest);
        out[114..146].copy_from_slice(&self.lot_commitment);
        out[146..148].copy_from_slice(&self.event_count.to_be_bytes());
        out[148] = u8::from(self.valid);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EpochError> {
        if bytes.len() != LEAF_PUBLIC_BYTES || bytes[148] > 1 {
            return Err(EpochError::InvalidInput);
        }
        Ok(Self {
            epoch_id: u64::from_be_bytes(bytes[..8].try_into().unwrap()),
            shipment_id: u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
            leaf_index: bytes[16],
            leaf_count: bytes[17],
            old_nullifier_root: bytes[18..50].try_into().unwrap(),
            new_nullifier_root: bytes[50..82].try_into().unwrap(),
            event_batch_digest: bytes[82..114].try_into().unwrap(),
            lot_commitment: bytes[114..146].try_into().unwrap(),
            event_count: u16::from_be_bytes(bytes[146..148].try_into().unwrap()),
            valid: bytes[148] == 1,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AggregatePublicValues {
    pub epoch_id: u64,
    pub old_nullifier_root: [u8; 32],
    pub new_nullifier_root: [u8; 32],
    pub epoch_commitment: [u8; 32],
    pub lot_count: u32,
    pub event_count: u32,
    pub valid: bool,
}

impl AggregatePublicValues {
    /// Solidity ABI: `(uint64,bytes32,bytes32,bytes32,uint32,uint32,bool)`.
    pub fn abi_encode(self) -> [u8; AGGREGATE_PUBLIC_BYTES] {
        let mut out = [0u8; AGGREGATE_PUBLIC_BYTES];
        out[24..32].copy_from_slice(&self.epoch_id.to_be_bytes());
        out[32..64].copy_from_slice(&self.old_nullifier_root);
        out[64..96].copy_from_slice(&self.new_nullifier_root);
        out[96..128].copy_from_slice(&self.epoch_commitment);
        out[156..160].copy_from_slice(&self.lot_count.to_be_bytes());
        out[188..192].copy_from_slice(&self.event_count.to_be_bytes());
        out[223] = u8::from(self.valid);
        out
    }
}

pub fn encode_aggregate_input(leaves: &[LeafPublicValues]) -> Result<Vec<u8>, EpochError> {
    if leaves.is_empty() || leaves.len() > MAX_FAN_IN {
        return Err(EpochError::InvalidInput);
    }
    let mut out = Vec::with_capacity(2 + leaves.len() * LEAF_PUBLIC_BYTES);
    out.push(1);
    out.push(leaves.len() as u8);
    for leaf in leaves {
        out.extend_from_slice(&leaf.encode());
    }
    Ok(out)
}

pub fn decode_aggregate_input(bytes: &[u8]) -> Result<Vec<LeafPublicValues>, EpochError> {
    if bytes.len() < 2
        || bytes[0] != 1
        || bytes[1] == 0
        || bytes[1] as usize > MAX_FAN_IN
        || bytes.len() != 2 + bytes[1] as usize * LEAF_PUBLIC_BYTES
    {
        return Err(EpochError::InvalidInput);
    }
    bytes[2..]
        .chunks_exact(LEAF_PUBLIC_BYTES)
        .map(LeafPublicValues::decode)
        .collect()
}

pub fn aggregate(leaves: &[LeafPublicValues]) -> Result<AggregatePublicValues, EpochError> {
    if leaves.is_empty() || leaves.len() > MAX_FAN_IN {
        return Err(EpochError::InvalidInput);
    }
    let epoch_id = leaves[0].epoch_id;
    let mut seed = Sha256::new();
    seed.update(b"EUDR:E3:EPOCH:V1");
    seed.update(epoch_id.to_be_bytes());
    let mut commitment: [u8; 32] = seed.finalize().into();
    let mut lot_count = 0u32;
    let mut event_count = 0u32;
    let mut previous: Option<&LeafPublicValues> = None;
    for leaf in leaves {
        if !leaf.valid
            || leaf.epoch_id != epoch_id
            || leaf.shipment_id == 0
            || leaf.leaf_count == 0
            || leaf.leaf_count as usize > MAX_FAN_IN
            || leaf.leaf_index >= leaf.leaf_count
            || !(8..=64).contains(&leaf.event_count)
        {
            return Err(EpochError::InvalidLeaf);
        }
        match previous {
            None if leaf.leaf_index != 0 => return Err(EpochError::InvalidOrder),
            Some(prev) => {
                if prev.new_nullifier_root != leaf.old_nullifier_root {
                    return Err(EpochError::RootMismatch);
                }
                if prev.shipment_id == leaf.shipment_id {
                    if prev.leaf_count != leaf.leaf_count || leaf.leaf_index != prev.leaf_index + 1
                    {
                        return Err(EpochError::InvalidOrder);
                    }
                } else if prev.leaf_index + 1 != prev.leaf_count
                    || leaf.shipment_id <= prev.shipment_id
                    || leaf.leaf_index != 0
                {
                    return Err(EpochError::InvalidOrder);
                }
            }
            _ => {}
        }
        if leaf.leaf_index == 0 {
            lot_count = lot_count.checked_add(1).ok_or(EpochError::Overflow)?;
        }
        event_count = event_count
            .checked_add(u32::from(leaf.event_count))
            .ok_or(EpochError::Overflow)?;
        let mut h = Sha256::new();
        h.update(b"EUDR:E3:EPOCH-FOLD:V1");
        h.update(commitment);
        h.update(leaf.lot_commitment);
        commitment = h.finalize().into();
        previous = Some(leaf);
    }
    if previous.unwrap().leaf_index + 1 != previous.unwrap().leaf_count {
        return Err(EpochError::InvalidOrder);
    }
    Ok(AggregatePublicValues {
        epoch_id,
        old_nullifier_root: leaves[0].old_nullifier_root,
        new_nullifier_root: leaves.last().unwrap().new_nullifier_root,
        epoch_commitment: commitment,
        lot_count,
        event_count,
        valid: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(index: u8, count: u8) -> LeafPublicValues {
        LeafPublicValues {
            epoch_id: 9,
            shipment_id: 17,
            leaf_index: index,
            leaf_count: count,
            old_nullifier_root: [index; 32],
            new_nullifier_root: [index + 1; 32],
            event_batch_digest: [3; 32],
            lot_commitment: [index + 8; 32],
            event_count: 64,
            valid: true,
        }
    }

    #[test]
    fn two_leaves_are_one_shipment() {
        let leaves = [leaf(0, 2), leaf(1, 2)];
        let encoded = encode_aggregate_input(&leaves).unwrap();
        assert_eq!(decode_aggregate_input(&encoded).unwrap(), leaves);
        let result = aggregate(&leaves).unwrap();
        assert_eq!((result.lot_count, result.event_count), (1, 128));
        assert_eq!(result.abi_encode().len(), AGGREGATE_PUBLIC_BYTES);
    }

    #[test]
    fn rejects_tampered_chain_and_order() {
        let first = leaf(0, 2);
        let mut second = leaf(1, 2);
        second.old_nullifier_root[0] ^= 1;
        assert_eq!(aggregate(&[first, second]), Err(EpochError::RootMismatch));
        assert_eq!(
            aggregate(&[leaf(1, 2), first]),
            Err(EpochError::InvalidOrder)
        );
        assert_eq!(aggregate(&[first, first]), Err(EpochError::RootMismatch));
        assert_eq!(aggregate(&[first]), Err(EpochError::InvalidOrder));
    }

    #[test]
    fn rejects_mixed_epoch_and_excess_fan_in() {
        let first = leaf(0, 2);
        let mut second = leaf(1, 2);
        second.epoch_id += 1;
        assert_eq!(aggregate(&[first, second]), Err(EpochError::InvalidLeaf));
        assert_eq!(
            aggregate(&[first; MAX_FAN_IN + 1]),
            Err(EpochError::InvalidInput)
        );
        assert_eq!(
            encode_aggregate_input(&[first; MAX_FAN_IN + 1]),
            Err(EpochError::InvalidInput)
        );
    }
}
