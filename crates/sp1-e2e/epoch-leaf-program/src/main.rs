#![no_main]
sp1_zkvm::entrypoint!(main);

pub fn main() {
    let bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let input = eudr_epoch_policy::LeafInput::decode(&bytes).expect("invalid E3 leaf input");
    let values = input.evaluate().expect("E3 leaf policy rejected");
    sp1_zkvm::io::commit_slice(&values.encode());
}
