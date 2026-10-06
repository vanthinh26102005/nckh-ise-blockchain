//! Issuer-side catalog; build snapshots before proving, never inside the prover.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credential {
    pub index: u16,
    pub event: EpcisEventV1,
    pub owner_identity: [u8; 32],
    pub key_commitment: [u8; 32],
}
#[derive(Default)]
pub struct RegistryIssuer {
    ownership: HashMap<u64, ([u8; 32], [u8; 32])>,
}
impl RegistryIssuer {
    /// Public ownership catalog persists across issuer restarts. Contains no private key.
    pub fn encode_catalog(&self) -> Vec<u8> {
        let mut lots:Vec<_>=self.ownership.iter().collect();lots.sort_by_key(|(lot,_)|**lot);
        let mut bytes=vec![2];bytes.extend_from_slice(&(lots.len() as u64).to_be_bytes());
        for (lot,(owner,key_commitment)) in lots { bytes.extend_from_slice(&lot.to_be_bytes());bytes.extend_from_slice(owner);bytes.extend_from_slice(key_commitment); } bytes
    }
    pub fn decode_catalog(bytes:&[u8]) -> Result<Self,PolicyError> {
        const MAX_ENTRIES:usize=65_536;
        if bytes.len()<9 || bytes.len()>9+MAX_ENTRIES*72 || bytes[0]!=2 {return Err(PolicyError::InvalidInput);}
        let count=u64::from_be_bytes(bytes[1..9].try_into().unwrap());
        if count>MAX_ENTRIES as u64 || bytes.len()!=9+count as usize*72 {return Err(PolicyError::InvalidInput);}
        let mut ownership=HashMap::new();
        for row in bytes[9..].chunks_exact(72) {
            let lot=u64::from_be_bytes(row[..8].try_into().unwrap());
            if ownership.insert(lot,(row[8..40].try_into().unwrap(),row[40..72].try_into().unwrap())).is_some(){return Err(PolicyError::InvalidInput);}
        }
        Ok(Self{ownership})
    }

    pub fn snapshot(&mut self, credentials: Vec<Credential>, polygon: Vec<PointE6>, polygon_nonce: [u8; 32]) -> Result<RegistrySnapshot, PolicyError> {
        if credentials.len() > 1_usize << CERTIFICATE_TREE_DEPTH { return Err(PolicyError::InvalidInput); }
        validate_polygon(&polygon)?;
        let mut proposed = self.ownership.clone();
        let mut slots = HashMap::new();
        for credential in &credentials {
            credential.event.point().validate()?;
            let ownership = (credential.owner_identity, credential.key_commitment);
            if proposed.get(&credential.event.lot_id).is_some_and(|old| *old != ownership)
                || slots.insert(credential.index, credential.clone()).is_some() {
                return Err(PolicyError::C2CredentialRoot);
            }
            proposed.insert(credential.event.lot_id, ownership);
        }
        if proposed.len() > 65_536 { return Err(PolicyError::InvalidInput); }
        let leaves = (0..1_usize << CERTIFICATE_TREE_DEPTH).map(|index| match slots.get(&(index as u16)) {
            Some(c) => certificate_leaf(index, &c.event, c.owner_identity, c.key_commitment),
            None => certificate_padding_leaf(index),
        }).collect::<Vec<_>>();
        let mut levels = vec![leaves];
        while levels.last().unwrap().len() > 1 {
            levels.push(levels.last().unwrap().chunks_exact(2).map(|p| certificate_parent(p[0], p[1])).collect());
        }
        self.ownership = proposed;
        Ok(RegistrySnapshot { credentials: slots, levels, polygon, polygon_nonce })
    }
}
#[derive(Clone)]
pub struct RegistrySnapshot {
    credentials: HashMap<u16, Credential>,
    levels: Vec<Vec<[u8; 32]>>,
    pub polygon: Vec<PointE6>,
    pub polygon_nonce: [u8; 32],
}
impl RegistrySnapshot {
    pub fn root(&self) -> [u8; 32] { self.levels.last().unwrap()[0] }
    pub fn credential(&self, event: &EpcisEventV1, owner: [u8; 32], key: [u8; 32]) -> Result<(u16, [[u8; 32]; CERTIFICATE_TREE_DEPTH]), PolicyError> {
        let key_commitment = nullifier_key_commitment(key);
        // ponytail: at most 65,536 slots; add a lookup index only if issuer-side preparation profiling warrants it.
        let credential = self.credentials.values().find(|c| c.event.lot_id == event.lot_id
            && c.event.actor_public_key == event.actor_public_key && c.event.certificate_id == event.certificate_id
            && c.event.role == event.role && c.owner_identity == owner && c.key_commitment == key_commitment)
            .ok_or(PolicyError::C2CredentialRoot)?;
        let mut slot = credential.index as usize;
        let path = std::array::from_fn(|depth| { let sibling = self.levels[depth][slot ^ 1]; slot >>= 1; sibling });
        Ok((credential.index, path))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn credential(lot_id: u64, epoch_id: u64) -> Credential {
        Credential { index: 0, event: EpcisEventV1 { event_id: 1, lot_id, epoch_id, timestamp_ms: 1,
            readings: 1, latitude_e6: 0, longitude_e6: 0, certificate_id: 1, role: 4, actor_public_key: [3;32] },
            owner_identity: [4;32], key_commitment: nullifier_key_commitment([5;32]) }
    }
    fn polygon() -> Vec<PointE6> { vec![PointE6::new(0,0), PointE6::new(0,10), PointE6::new(10,0)] }

    #[test]
    fn restored_issuer_accepts_same_identity_in_later_epoch_but_rejects_rotation() {
        let mut original = RegistryIssuer::default();
        original.snapshot(vec![credential(2,1)], polygon(), [1;32]).unwrap();
        let saved = original.encode_catalog();
        let mut restored = RegistryIssuer::decode_catalog(&saved).unwrap();
        let next = credential(2,2);
        restored.snapshot(vec![next.clone()], polygon(), [2;32]).unwrap();
        let mut changed_owner = next.clone(); changed_owner.owner_identity[0] ^= 1;
        assert!(matches!(restored.snapshot(vec![changed_owner], polygon(), [2;32]), Err(PolicyError::C2CredentialRoot)));
        let mut changed_key = next; changed_key.key_commitment[0] ^= 1;
        assert!(matches!(restored.snapshot(vec![changed_key], polygon(), [2;32]), Err(PolicyError::C2CredentialRoot)));
        assert_eq!(restored.encode_catalog(), saved); // Rejected issuance cannot mutate ownership.
    }

    #[test]
    fn catalog_rejects_old_version_truncation_and_duplicate_lots() {
        let issuer = RegistryIssuer { ownership: [(2, ([4;32], [5;32]))].into_iter().collect() };
        let bytes = issuer.encode_catalog();
        let mut old = bytes.clone(); old[0] = 1;
        assert!(matches!(RegistryIssuer::decode_catalog(&old), Err(PolicyError::InvalidInput)));
        for end in [0, 1, 8, 9, bytes.len()-1] {
            assert!(matches!(RegistryIssuer::decode_catalog(&bytes[..end]), Err(PolicyError::InvalidInput)));
        }
        let mut duplicate = bytes.clone(); duplicate[1..9].copy_from_slice(&2_u64.to_be_bytes());
        duplicate.extend_from_slice(&bytes[9..]);
        assert!(matches!(RegistryIssuer::decode_catalog(&duplicate), Err(PolicyError::InvalidInput)));
    }

    #[test]
    fn cumulative_catalog_limit_rejects_new_lot_without_losing_old_records() {
        // Seed the private test fixture directly; avoid constructing 65,536 certificate proofs.
        let mut issuer = RegistryIssuer { ownership: (0..65_536_u64).map(|lot| (lot, ([4;32], [5;32]))).collect() };
        let saved = issuer.encode_catalog();
        assert_eq!(RegistryIssuer::decode_catalog(&saved).unwrap().encode_catalog(), saved);
        assert!(matches!(issuer.snapshot(vec![credential(65_536,2)], polygon(), [2;32]), Err(PolicyError::InvalidInput)));
        assert_eq!(issuer.encode_catalog(), saved);
        assert_eq!(RegistryIssuer::decode_catalog(&issuer.encode_catalog()).unwrap().ownership.len(), 65_536);
    }

    #[test]
    fn issuer_preserves_ownership_across_snapshots_and_rejects_duplicate_slots() {
        let event = EpcisEventV1 { event_id: 1, lot_id: 2, epoch_id: 1, timestamp_ms: 1, readings: 1, latitude_e6: 0, longitude_e6: 0, certificate_id: 1, role: 4, actor_public_key: [3;32] };
        let c = Credential { index: 0, event, owner_identity: [4;32], key_commitment: nullifier_key_commitment([5;32]) };
        let polygon = vec![PointE6::new(0,0), PointE6::new(0,10), PointE6::new(10,0)];
        let mut issuer = RegistryIssuer::default();
        let snapshot = issuer.snapshot(vec![c.clone()], polygon.clone(), [1;32]).unwrap();
        let (index,path) = snapshot.credential(&event, [4;32], [5;32]).unwrap();
        assert_eq!(credential_root(index as usize, &event, [4;32], nullifier_key_commitment([5;32]), &path), snapshot.root());
        assert!(snapshot.credential(&event, [4;32], [6;32]).is_err());
        let saved=issuer.encode_catalog();
        issuer=RegistryIssuer::decode_catalog(&saved).unwrap();
        let mut trailing=saved.clone();trailing.push(0);assert!(RegistryIssuer::decode_catalog(&trailing).is_err());
        let mut changed = c.clone(); changed.key_commitment = [9;32];
        assert!(issuer.snapshot(vec![changed], polygon.clone(), [1;32]).is_err());
        assert!(issuer.snapshot(vec![c.clone(),c], polygon, [1;32]).is_err());
    }
}
