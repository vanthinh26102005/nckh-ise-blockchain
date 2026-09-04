#![no_main]
sp1_zkvm::entrypoint!(main);

pub fn main() {
    let bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let input = eudr_policy::PolicyInput::decode(&bytes).expect("invalid private policy input");
    let public_values = eudr_policy::evaluate(&input).expect("EUDR policy rejected private input");
    sp1_zkvm::io::commit_slice(&public_values.abi_encode());
}
