fn main() {
    let output = std::env::var("SP1_FIXTURE_OUT")
        .unwrap_or_else(|_| "/tmp/eudr-sp1-groth16-fixture.json".to_string());
    let fixture = sp1_e2e::prove_evm_fixture(&sp1_e2e::valid_input())
        .expect("local CPU Groth16 proof failed");
    std::fs::write(&output, fixture).expect("write proof fixture");
    println!("wrote proof fixture to {output}");
}
