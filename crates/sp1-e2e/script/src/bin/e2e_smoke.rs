use anyhow::{anyhow, bail, Context, Result};
use sp1_e2e::{fixture_input, http::FabricGateway, prove_evm_fixture};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<()> {
    let epoch_id = env_u64("E2E_EPOCH")?.unwrap_or_else(default_epoch_id);
    let first_event_id = env_u64("E2E_EVENT_ID_BASE")?.unwrap_or(
        epoch_id
            .checked_mul(100)
            .expect("epoch ID fits event ID range"),
    );
    let endpoint = fabric_endpoint()?;
    let input = fixture_input(epoch_id, first_event_id);

    for witness in &input.events {
        endpoint.post_event(&witness.event_bytes)?;
    }
    for witness in &input.events {
        endpoint.assert_event_was_committed(&witness.event_bytes)?;
    }

    let proof_start = Instant::now();
    let fixture = prove_evm_fixture(&input).map_err(|error| anyhow!(error))?;
    let proof_ms = proof_start.elapsed().as_millis();
    let fixture_path = env::var("E2E_PROOF_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("/tmp/eudr-e2e-proof-{epoch_id}.json")));
    fs::write(&fixture_path, &fixture)
        .with_context(|| format!("write proof fixture {}", fixture_path.display()))?;

    let anchor_start = Instant::now();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("script manifest is nested under the repository root")
        .to_path_buf();
    let status = Command::new("npm")
        .args(["run", "anvil:smoke"])
        .current_dir(repo_root.join("contracts"))
        .env("SP1_FIXTURE", &fixture_path)
        .env(
            "ANVIL_RPC",
            env::var("ANVIL_RPC").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string()),
        )
        .status()
        .context("run SP1 proof verification and anchor transaction on Anvil")?;
    if !status.success() {
        bail!("Anvil SP1 verifier smoke failed with {status}");
    }
    let anchor_ms = anchor_start.elapsed().as_millis();

    let proof_bytes = fixture_proof_bytes(&fixture)?;
    println!(
        "{{\n  \"epochId\": {epoch_id},\n  \"firstEventId\": {first_event_id},\n  \"fabricEventsAcknowledged\": {},\n  \"proofMsAfterFabricAcknowledgement\": {proof_ms},\n  \"anvilAnchorMs\": {anchor_ms},\n  \"fabricAcknowledgementToReceiptMs\": {},\n  \"proofBytes\": {proof_bytes},\n  \"proofFixture\": \"{}\"\n}}",
        input.events.len(),
        proof_ms + anchor_ms,
        fixture_path.display(),
    );
    Ok(())
}

fn env_u64(name: &str) -> Result<Option<u64>> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{name} must be an unsigned integer"))
            .map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {name}")),
    }
}

fn default_epoch_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after Unix epoch")
        .as_secs()
}

fn fabric_endpoint() -> Result<FabricGateway> {
    let value =
        env::var("FABRIC_GATEWAY_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    FabricGateway::from_url(&value)
}

fn fixture_proof_bytes(fixture: &str) -> Result<usize> {
    let parsed = serde_json::from_str::<serde_json::Value>(fixture)?;
    let proof = parsed["proof"]
        .as_str()
        .ok_or_else(|| anyhow!("SP1 proof fixture has no proof string"))?;
    let hex = proof.strip_prefix("0x").unwrap_or(proof);
    if hex.len() % 2 != 0 {
        bail!("SP1 proof fixture has an odd hex length");
    }
    Ok(hex.len() / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_byte_count_rejects_odd_hex() {
        assert!(fixture_proof_bytes(r#"{\"proof\":\"0x0\"}"#).is_err());
    }
}
