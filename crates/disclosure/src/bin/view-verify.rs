use eudr_disclosure::{
    cli, parse_hex,
    rpc::{verify_on_chain, TrustConfig},
    verify_openings, Bundle,
};
use std::time::Instant;

fn run() -> Result<(), String> {
    let args = cli::args(&["--bundle", "--trust", "--rpc"])?;
    if args.contains_key("--help") {
        println!("view-verify --bundle view.json --trust independently-prepared-trust.json --rpc https://expected-chain-rpc");
        return Ok(());
    }
    let bytes = cli::read(cli::required(&args, "--bundle")?)?;
    let bundle: Bundle = cli::json(&bytes)?;
    let trust: TrustConfig = cli::json(&cli::read(cli::required(&args, "--trust")?)?)?;
    let start = Instant::now();
    let verified = verify_openings(
        &bundle,
        parse_hex(&trust.leaf_v_key_digest)?,
        parse_hex(&trust.aggregate_v_key_digest)?,
    )?;
    let opening_us = start.elapsed().as_micros();
    let rpc_start = Instant::now();
    verify_on_chain(&bundle, &trust, cli::required(&args, "--rpc")?)?;
    println!(
        "{}",
        serde_json::json!({"protocolVersion":2,"status":"verified-openings-and-anchor","measurementKind":"selective-disclosure-prototype",
        "formal":false,"lotId":verified.lot_id,"epochId":verified.epoch_id,"bundleBytes":bytes.len(),"openingVerifyMicros":opening_us,
        "onChainVerifyMicros":rpc_start.elapsed().as_micros(),"official30Repetitions":"pending"})
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("view-verify: {error}");
        std::process::exit(1);
    }
}
