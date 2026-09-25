#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

pub fn main() {
    let bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let input = eudr_policy::epoch::EpochInput::decode(&bytes).expect("invalid epoch input");
    for child in &input.children {
        let (key, values): (&[u8; 32], &[u8]) = match child {
            eudr_policy::epoch::EpochChild::Leaf(value) => (&input.leaf_vk_digest, value),
            eudr_policy::epoch::EpochChild::Aggregate(value) => (&input.aggregate_vk_digest, value),
        };
        let digest: [u8; 32] = Sha256::digest(values).into();
        sp1_zkvm::lib::verify::verify_sp1_proof(
            &eudr_policy::epoch::EpochInput::vk_words(key),
            &digest,
        );
    }
    let public_values = eudr_policy::epoch::evaluate(&input).expect("invalid epoch chain");
    sp1_zkvm::io::commit_slice(&public_values.abi_encode());
}
