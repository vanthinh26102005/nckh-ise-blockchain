#![no_main]
use sha2::{Digest, Sha256};
sp1_zkvm::entrypoint!(main);

pub fn main() {
    let input = sp1_zkvm::io::read::<Vec<u8>>();
    let leaves =
        eudr_epoch_policy::decode_aggregate_input(&input).expect("invalid aggregate input");
    // The build script derives this digest from the exact leaf ELF before
    // compiling this program. It is part of the aggregate program vkey.
    let words = option_env!("E3_LEAF_VKEY_WORDS").expect("leaf vkey not embedded by build");
    let vkey: [u32; 8] = words
        .split(',')
        .map(|word| word.parse::<u32>().expect("bad embedded leaf vkey"))
        .collect::<Vec<_>>()
        .try_into()
        .expect("leaf vkey must have eight words");

    for leaf in &leaves {
        let digest: [u8; 32] = Sha256::digest(leaf.encode()).into();
        sp1_zkvm::lib::verify::verify_sp1_proof(&vkey, &digest);
    }
    let values = eudr_epoch_policy::aggregate(&leaves).expect("invalid epoch transition");
    sp1_zkvm::io::commit_slice(&values.abi_encode());
}
