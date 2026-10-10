use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct HttpEndpoint {
    address: String,
    host_header: String,
}

impl HttpEndpoint {
    pub fn from_url(url: &str) -> Result<Self> {
        let authority = url
            .strip_prefix("http://")
            .ok_or_else(|| anyhow!("endpoint must use http://"))?;
        if authority.is_empty() || authority.contains('/') {
            bail!("endpoint must be an http://host:port URL without a path");
        }
        Ok(Self {
            address: authority.to_string(),
            host_header: authority.to_string(),
        })
    }

    pub fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<HttpResponse> {
        let mut stream = TcpStream::connect(&self.address)
            .with_context(|| format!("connect to HTTP endpoint {}", self.address))?;
        stream.set_read_timeout(Some(Duration::from_secs(180)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        let body = body.unwrap_or("");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            self.host_header,
            body.len(),
        )?;
        stream.flush()?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        HttpResponse::parse(&response)
    }
}

#[derive(Clone, Debug)]
pub struct FabricGateway(HttpEndpoint);

impl FabricGateway {
    pub fn from_url(url: &str) -> Result<Self> {
        Ok(Self(HttpEndpoint::from_url(url)?))
    }

    pub fn post_event(&self, event: &[u8; 86]) -> Result<()> {
        let body = format!(
            "{{\"canonicalEvent\":\"{}\",\"digest\":\"{}\"}}",
            STANDARD.encode(event),
            hex::encode(Sha256::digest(event)),
        );
        let response = self.0.request("POST", "/events", Some(&body))?;
        if response.status != 201 {
            bail!(
                "Fabric did not acknowledge event {}: {}",
                event_id(event),
                response.body
            );
        }
        Ok(())
    }

    pub fn assert_event_was_committed(&self, event: &[u8; 86]) -> Result<()> {
        self.read_committed_event(event).map(|_| ())
    }

    /// Read the ledger record and return the exact canonical bytes checked against the submission.
    pub fn read_committed_event(&self, expected: &[u8; 86]) -> Result<[u8; 86]> {
        let response = self
            .0
            .request("GET", &format!("/events/{}", event_id(expected)), None)?;
        parse_committed_event(&response, expected)
    }
}

fn parse_committed_event(response: &HttpResponse, expected: &[u8; 86]) -> Result<[u8; 86]> {
    if response.status != 200 {
        bail!("Fabric ledger readback failed for event {}: HTTP {}", event_id(expected), response.status);
    }
    let record: serde_json::Value = serde_json::from_str(&response.body)
        .context("parse Fabric ledger event record")?;
    if record["eventId"].as_str() != Some(event_id(expected).to_string().as_str()) {
        bail!("Fabric ledger readback event ID differs from submitted event");
    }
    let encoded = record["canonicalEvent"].as_str()
        .ok_or_else(|| anyhow!("Fabric ledger readback omits canonicalEvent"))?;
    let bytes: [u8; 86] = STANDARD.decode(encoded)
        .context("decode Fabric ledger canonicalEvent")?
        .try_into()
        .map_err(|_| anyhow!("Fabric ledger canonicalEvent is not 86 bytes"))?;
    if bytes != *expected {
        bail!("Fabric ledger canonicalEvent differs from submitted event {}", event_id(expected));
    }
    let digest = hex::encode(Sha256::digest(bytes));
    if record["digest"].as_str() != Some(digest.as_str()) {
        bail!("Fabric ledger digest differs from canonical event {}", event_id(expected));
    }
    Ok(bytes)
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn parse(response: &str) -> Result<Self> {
        let (head, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| anyhow!("HTTP endpoint returned malformed response"))?;
        let status = head
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow!("HTTP endpoint response had no status"))?
            .parse()
            .context("parse HTTP status")?;
        Ok(Self {
            status,
            body: body.to_string(),
        })
    }
}

pub fn event_id(event: &[u8; 86]) -> u64 {
    u64::from_be_bytes(event[1..9].try_into().expect("canonical event ID width"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_error_body() {
        let response = HttpResponse::parse(
            "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\n\r\n{\"error\":\"duplicate\"}",
        )
        .unwrap();
        assert_eq!(response.status, 409);
        assert_eq!(response.body, r#"{"error":"duplicate"}"#);
    }

    #[test]
    fn endpoint_rejects_paths_and_https() {
        assert!(HttpEndpoint::from_url("https://127.0.0.1:8080").is_err());
        assert!(HttpEndpoint::from_url("http://127.0.0.1:8080/path").is_err());
    }

    #[test]
    fn ledger_readback_requires_exact_id_bytes_and_digest() {
        let mut event = [0u8; 86];
        event[0] = 1;
        event[8] = 7;
        let record = serde_json::json!({"eventId":"7","canonicalEvent":STANDARD.encode(event),
            "digest":hex::encode(Sha256::digest(event))});
        let response = HttpResponse { status: 200, body: record.to_string() };
        assert_eq!(parse_committed_event(&response, &event).unwrap(), event);
        let mut wrong_id = record.clone();
        wrong_id["eventId"] = serde_json::json!("8");
        assert!(parse_committed_event(&HttpResponse { status:200, body:wrong_id.to_string() }, &event).is_err());
        let mut wrong_bytes = record.clone();
        let mut other = event;
        other[85] = 1;
        wrong_bytes["canonicalEvent"] = serde_json::json!(STANDARD.encode(other));
        assert!(parse_committed_event(&HttpResponse { status:200, body:wrong_bytes.to_string() }, &event).is_err());
        let mut wrong_digest = record;
        wrong_digest["digest"] = serde_json::json!("00");
        assert!(parse_committed_event(&HttpResponse { status:200, body:wrong_digest.to_string() }, &event).is_err());
    }
}
