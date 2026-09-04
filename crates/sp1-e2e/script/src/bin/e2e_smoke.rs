use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use sp1_e2e::{fixture_input, prove_evm_fixture};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct FabricEndpoint {
    address: String,
    host_header: String,
}

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
        post_event(&endpoint, &witness.event_bytes)?;
    }
    for witness in &input.events {
        assert_event_was_committed(&endpoint, &witness.event_bytes)?;
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

fn fabric_endpoint() -> Result<FabricEndpoint> {
    let value =
        env::var("FABRIC_GATEWAY_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let authority = value
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("FABRIC_GATEWAY_URL must use http://"))?;
    if authority.is_empty() || authority.contains('/') {
        bail!("FABRIC_GATEWAY_URL must be an http://host:port endpoint");
    }
    Ok(FabricEndpoint {
        address: authority.to_string(),
        host_header: authority.to_string(),
    })
}

fn post_event(endpoint: &FabricEndpoint, event: &[u8; 86]) -> Result<()> {
    let body = format!(
        "{{\"canonicalEvent\":\"{}\",\"digest\":\"{}\"}}",
        STANDARD.encode(event),
        hex::encode(Sha256::digest(event)),
    );
    let response = http_request(endpoint, "POST", "/events", Some(&body))?;
    if response.status != 201 {
        bail!(
            "Fabric did not acknowledge event {}: {}",
            event_id(event),
            response.body
        );
    }
    Ok(())
}

fn assert_event_was_committed(endpoint: &FabricEndpoint, event: &[u8; 86]) -> Result<()> {
    let canonical_event = STANDARD.encode(event);
    let response = http_request(
        endpoint,
        "GET",
        &format!("/events/{}", event_id(event)),
        None,
    )?;
    if response.status != 200 || !response.body.contains(&canonical_event) {
        bail!(
            "Fabric ledger query did not return canonical event {}",
            event_id(event)
        );
    }
    Ok(())
}

struct HttpResponse {
    status: u16,
    body: String,
}

fn http_request(
    endpoint: &FabricEndpoint,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<HttpResponse> {
    let mut stream = TcpStream::connect(&endpoint.address)
        .with_context(|| format!("connect to Fabric gateway {}", endpoint.address))?;
    stream.set_read_timeout(Some(Duration::from_secs(90)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        endpoint.host_header,
        body.len(),
    )?;
    stream.flush()?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    parse_http_response(&response)
}

fn parse_http_response(response: &str) -> Result<HttpResponse> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("Fabric gateway returned malformed HTTP response"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("Fabric gateway response had no status"))?
        .parse()
        .context("parse Fabric gateway HTTP status")?;
    Ok(HttpResponse {
        status,
        body: body.to_string(),
    })
}

fn event_id(event: &[u8; 86]) -> u64 {
    u64::from_be_bytes(event[1..9].try_into().expect("canonical event ID width"))
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
    fn event_id_reads_the_canonical_big_endian_field() {
        let mut event = [0_u8; 86];
        event[1..9].copy_from_slice(&9_000_000_100_u64.to_be_bytes());
        assert_eq!(event_id(&event), 9_000_000_100);
    }

    #[test]
    fn proof_byte_count_rejects_odd_hex() {
        assert!(fixture_proof_bytes(r#"{\"proof\":\"0x0\"}"#).is_err());
    }

    #[test]
    fn http_response_parser_preserves_gateway_error_body() {
        let response = parse_http_response(
            "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\n\r\n{\"error\":\"duplicate\"}",
        )
        .unwrap();
        assert_eq!(response.status, 409);
        assert_eq!(response.body, r#"{"error":"duplicate"}"#);
    }
}
