//! Bind openings to a mined aggregate transaction using independently supplied trust.
use crate::{parse_hex, verify_openings, Bundle};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};

const EPOCH_TOPIC: &str = "0xe22c7f549066ee73d713f967a0f35cf2990f5e14f1c9b9aae9a565791ed3eacb";
const VERIFIER_HASH: &str = "0x4388a21c687fdd5f218d7e3d13190cac4c5355818d3605fd5fb811df468ee696";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyPair {
    pub certificate_root: String,
    pub polygon_commitment: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustConfig {
    pub protocol_version: u8,
    pub chain_id: u64,
    pub anchor_address: String,
    pub registry_address: String,
    pub verifier_address: String,
    pub verifier_code_sha256: String,
    pub program_v_key: String,
    pub leaf_v_key_digest: String,
    pub aggregate_v_key_digest: String,
    pub accepted_policies: Vec<PolicyPair>,
}

fn address(value: &str) -> Result<String, String> {
    Ok(crate::encode_hex(parse_hex::<20>(value)?))
}
fn hex_number(value: &Value) -> Result<u64, String> {
    u64::from_str_radix(
        value
            .as_str()
            .and_then(|s| s.strip_prefix("0x"))
            .ok_or("missing RPC hex integer")?,
        16,
    )
    .map_err(|_| "invalid RPC integer".into())
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("RPC response is missing {field}"))
}
fn call(client: &Client, url: &str, method: &str, params: Value) -> Result<Value, String> {
    let response = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .map_err(|_| "RPC request failed")?
        .error_for_status()
        .map_err(|_| "RPC HTTP request failed")?;
    let mut body = String::new();
    response
        .take(1_000_001)
        .read_to_string(&mut body)
        .map_err(|_| "RPC response could not be read")?;
    if body.len() > 1_000_000 {
        return Err("RPC response is too large".into());
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| "invalid RPC response JSON")?;
    if value.get("error").is_some() {
        return Err("RPC returned an error".into());
    }
    value
        .get("result")
        .cloned()
        .filter(|v| !v.is_null())
        .ok_or_else(|| "RPC result is unavailable".into())
}
fn getter(
    client: &Client,
    url: &str,
    target: &str,
    data: &str,
    block: &str,
) -> Result<[u8; 32], String> {
    let value = call(
        client,
        url,
        "eth_call",
        json!([{"to":target,"data":data},block]),
    )?;
    parse_hex(value.as_str().ok_or("RPC call did not return hex")?)
}
fn address_word(word: [u8; 32]) -> Result<String, String> {
    if word[..12] != [0; 12] {
        return Err("RPC address word is not canonical".into());
    }
    Ok(crate::encode_hex(&word[12..]))
}
fn code(client: &Client, url: &str, target: &str, block: &str) -> Result<Vec<u8>, String> {
    let value = call(client, url, "eth_getCode", json!([target, block]))?;
    let value = value
        .as_str()
        .and_then(|v| v.strip_prefix("0x"))
        .ok_or("RPC code is not hex")?;
    let bytes = hex::decode(value).map_err(|_| "RPC code is not hex")?;
    if bytes.is_empty() {
        return Err("expected contract has no deployed code".into());
    }
    Ok(bytes)
}

pub fn verify_on_chain(bundle: &Bundle, trust: &TrustConfig, url: &str) -> Result<(), String> {
    if trust.protocol_version != 2 {
        return Err("external trust configuration must use protocolVersion 2".into());
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| "invalid RPC URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("RPC URL must use HTTP or HTTPS".into());
    }
    let leaf_vk = parse_hex::<32>(&trust.leaf_v_key_digest)?;
    let aggregate_vk = parse_hex::<32>(&trust.aggregate_v_key_digest)?;
    let verified = verify_openings(bundle, leaf_vk, aggregate_vk)?;
    let root = verified.root_public_values;
    let anchor = address(&trust.anchor_address)?;
    let registry = address(&trust.registry_address)?;
    let verifier = address(&trust.verifier_address)?;
    let program_vk = parse_hex::<32>(&trust.program_v_key)?;
    let expected_code = parse_hex::<32>(&trust.verifier_code_sha256)?;
    let certificate: [u8; 32] = root[288..320].try_into().unwrap();
    let polygon: [u8; 32] = root[320..352].try_into().unwrap();
    if !trust.accepted_policies.iter().any(|pair| {
        parse_hex::<32>(&pair.certificate_root) == Ok(certificate)
            && parse_hex::<32>(&pair.polygon_commitment) == Ok(polygon)
    }) {
        return Err("root policy is not trusted by the external configuration".into());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "RPC client initialization failed")?;
    if hex_number(&call(&client, url, "eth_chainId", json!([]))?)? != trust.chain_id {
        return Err("RPC chain does not match expected chainId".into());
    }
    let transaction_hash = &bundle.authentication.transaction_hash;
    let receipt = call(
        &client,
        url,
        "eth_getTransactionReceipt",
        json!([transaction_hash]),
    )?;
    if hex_number(&receipt["status"])? != 1
        || text(&receipt, "transactionHash")?.to_lowercase() != transaction_hash.to_lowercase()
        || address(text(&receipt, "to")?)? != anchor
    {
        return Err("receipt is not a successful transaction to the expected anchor".into());
    }
    let block = text(&receipt, "blockNumber")?;
    hex_number(&receipt["blockNumber"])?;
    let block_hash = parse_hex::<32>(text(&receipt, "blockHash")?)?;
    let mined_block = call(&client, url, "eth_getBlockByNumber", json!([block, false]))?;
    if parse_hex::<32>(text(&mined_block, "hash")?)? != block_hash {
        return Err("receipt block is no longer canonical".into());
    }
    let tx = call(
        &client,
        url,
        "eth_getTransactionByHash",
        json!([transaction_hash]),
    )?;
    if address(text(&tx, "to")?)? != anchor
        || text(&tx, "hash")?.to_lowercase() != transaction_hash.to_lowercase()
        || parse_hex::<32>(text(&tx, "blockHash")?)? != block_hash
    {
        return Err("transaction is not bound to the expected anchor and receipt".into());
    }
    let input = text(&tx, "input")?;
    let data = hex::decode(
        input
            .strip_prefix("0x")
            .ok_or("transaction input is not hex")?,
    )
    .map_err(|_| "transaction input is not hex")?;
    // Canonical verifyAndAnchor(uint64,bytes32,bytes,bytes), 352 public bytes + 356 proof bytes.
    if data.len() != 932 || data[..4] != [0xfe, 0xba, 0xb2, 0x45] {
        return Err("transaction is not a canonical policy-v2 aggregate call".into());
    }
    let args = &data[4..];
    let number_word = |offset: usize, expected: usize| {
        args[offset..offset + 24] == [0; 24]
            && u64::from_be_bytes(args[offset + 24..offset + 32].try_into().unwrap())
                == expected as u64
    };
    if args[..32] != root[..32]
        || args[32..64] != root[32..64]
        || !number_word(64, 128)
        || !number_word(96, 512)
        || !number_word(128, 352)
        || args[160..512] != root
        || !number_word(512, 356)
        || args[544..548] != [0x43, 0x88, 0xa2, 0x1c]
        || args[900..928] != [0; 28]
    {
        return Err(
            "transaction public values or proof encoding do not match the authenticated epoch"
                .into(),
        );
    }
    let logs = receipt["logs"].as_array().ok_or("receipt has no logs")?;
    let matching = logs
        .iter()
        .filter(|log| {
            log.get("address")
                .and_then(Value::as_str)
                .is_some_and(|v| v.to_lowercase() == anchor)
                && log["topics"][0].as_str() == Some(EPOCH_TOPIC)
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err("receipt must contain exactly one expected EpochAggregated log".into());
    }
    let log = matching[0];
    if log.get("removed").and_then(Value::as_bool) == Some(true)
        || text(log, "transactionHash")?.to_lowercase() != transaction_hash.to_lowercase()
        || parse_hex::<32>(text(log, "blockHash")?)? != block_hash
    {
        return Err("aggregate log is not bound to the mined receipt".into());
    }
    let topics = log["topics"]
        .as_array()
        .ok_or("aggregate log topics are missing")?;
    if topics.len() != 3
        || parse_hex::<32>(topics[1].as_str().ok_or("invalid epoch topic")?)? != root[..32]
        || parse_hex::<32>(topics[2].as_str().ok_or("invalid commitment topic")?)? != root[96..128]
    {
        return Err(
            "aggregate log epoch or commitment does not match the authenticated path".into(),
        );
    }
    let log_data = parse_hex::<128>(text(log, "data")?)?;
    if log_data[..32] != root[32..64]
        || log_data[32..64] != root[64..96]
        || log_data[64..96] != root[128..160]
        || log_data[96..128] != root[160..192]
    {
        return Err("aggregate log roots or counts do not match the authenticated path".into());
    }
    code(&client, url, &anchor, block)?;
    if getter(&client, url, &anchor, "0x1ddb5d29", block)? != program_vk
        || getter(&client, url, &anchor, "0x19f17e13", block)? != leaf_vk
        || getter(&client, url, &anchor, "0x2dd996f2", block)? != aggregate_vk
    {
        return Err("anchor program keys do not match external trust".into());
    }
    if address_word(getter(&client, url, &anchor, "0x1c4dd7d0", block)?)? != registry
        || address_word(getter(&client, url, &anchor, "0x2b7ac3f3", block)?)? != verifier
    {
        return Err("anchor registry or verifier does not match external trust".into());
    }
    let proof_code = code(&client, url, &verifier, block)?;
    let actual_code: [u8; 32] = Sha256::digest(&proof_code).into();
    if actual_code != expected_code
        || getter(&client, url, &verifier, "0x2a510436", block)? != parse_hex::<32>(VERIFIER_HASH)?
    {
        return Err(
            "verifier code does not match the independently trusted real SP1 verifier".into(),
        );
    }
    let allowed = getter(
        &client,
        url,
        &registry,
        &format!(
            "0x6b2a3ecf{}{}",
            hex::encode(certificate),
            hex::encode(polygon)
        ),
        block,
    )?;
    if allowed[..31] != [0; 31] || allowed[31] != 1 {
        return Err("policy roots were not registered at the anchoring block".into());
    }
    Ok(())
}
