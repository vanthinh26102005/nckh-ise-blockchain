//! Bounded leaf execution only: never calls prove or sets up proving keys.
use anyhow::{anyhow, bail, Result};
use eudr_policy::PolicyInput;
use sp1_e2e::{valid_input,EUDR_POLICY_ELF};
use sp1_sdk::{blocking::{Prover,ProverClient},SP1Stdin};
fn main() -> Result<()> {
    let input = match std::env::args().nth(1) {
        Some(path) => { if std::fs::metadata(&path)?.len()>51_155 {bail!("private witness exceeds v2 size bound");} PolicyInput::decode(&std::fs::read(path)?).map_err(|e|anyhow!("input: {e:?}"))? },
        None => valid_input(),
    };
    if input.events.len()!=8 { bail!("local execution permits exactly eight events"); }
    let expected=eudr_policy::evaluate(&input).map_err(|e|anyhow!("host: {e:?}"))?.abi_encode();
    let mut stdin=SP1Stdin::new();stdin.write(&input.encode().map_err(|e|anyhow!("encode: {e:?}"))?);
    eprintln!("host policy verified; initializing execute-only SP1 client");
    let client=ProverClient::builder().light().build();
    eprintln!("executing eight-event guest with 50M cycle limit");
    let (values,report)=client.execute(EUDR_POLICY_ELF,stdin).cycle_limit(50_000_000).calculate_gas(false).run()?;
    if values.as_slice()!=expected { bail!("guest public values differ from host"); }
    println!("{}",serde_json::json!({"protocolVersion":2,"validationKind":"guest-execute-only","events":8,
        "publicValues":format!("0x{}",hex::encode(values.as_slice())),"cycles": if report.total_instruction_count()==0 { None } else { Some(report.total_instruction_count()) }, "cycleLimit":50_000_000,"proofGenerated":false}));
    Ok(())
}
