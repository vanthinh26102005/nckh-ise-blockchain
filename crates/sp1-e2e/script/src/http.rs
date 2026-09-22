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
        let response = self
            .0
            .request("GET", &format!("/events/{}", event_id(event)), None)?;
        if response.status != 200 || !response.body.contains(&STANDARD.encode(event)) {
            bail!(
                "Fabric ledger query did not return canonical event {}",
                event_id(event)
            );
        }
        Ok(())
    }
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
}
