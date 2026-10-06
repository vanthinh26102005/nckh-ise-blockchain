//! JSON-RPC fixtures test binding only: no real proof or formal receipt is created.
mod common;
use eudr_disclosure::{
    encode_hex, open_bundle, parse_fields,
    rpc::{verify_on_chain, PolicyPair, TrustConfig},
    verify_openings,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

fn address(byte: u8) -> String {
    encode_hex([byte; 20])
}
fn word(number: u64) -> Vec<u8> {
    let mut value = vec![0; 32];
    value[24..].copy_from_slice(&number.to_be_bytes());
    value
}
fn address_word(byte: u8) -> String {
    let mut value = [0; 32];
    value[12..].fill(byte);
    encode_hex(value)
}
fn records(bundle: &eudr_disclosure::Bundle) -> (TrustConfig, Vec<Value>) {
    let root = verify_openings(bundle, [21; 32], [22; 32])
        .unwrap()
        .root_public_values;
    let anchor = address(30);
    let registry = address(31);
    let verifier = address(32);
    let block_hash = encode_hex([40; 32]);
    let transaction_hash = &bundle.authentication.transaction_hash;
    let trust = TrustConfig {
        protocol_version: 2,
        chain_id: 31337,
        anchor_address: anchor.clone(),
        registry_address: registry,
        verifier_address: verifier,
        verifier_code_sha256: encode_hex(Sha256::digest([0x60, 0x00])),
        program_v_key: encode_hex([23; 32]),
        leaf_v_key_digest: encode_hex([21; 32]),
        aggregate_v_key_digest: encode_hex([22; 32]),
        accepted_policies: vec![PolicyPair {
            certificate_root: encode_hex(&root[288..320]),
            polygon_commitment: encode_hex(&root[320..352]),
        }],
    };
    let mut input = vec![0xfe, 0xba, 0xb2, 0x45];
    input.extend(&root[..64]);
    input.extend(word(128));
    input.extend(word(512));
    input.extend(word(352));
    input.extend(root);
    input.extend(word(356));
    input.extend([0x43, 0x88, 0xa2, 0x1c]);
    input.extend([0; 352]);
    input.extend([0; 28]);
    assert_eq!(input.len(), 932);
    let mut log_data = vec![];
    log_data.extend(&root[32..96]);
    log_data.extend(&root[128..192]);
    let receipt = json!({"status":"0x1","transactionHash":transaction_hash,"to":anchor,"blockNumber":"0x10","blockHash":block_hash,
        "logs":[{"address":anchor,"transactionHash":transaction_hash,"blockHash":block_hash,"removed":false,
            "topics":["0xe22c7f549066ee73d713f967a0f35cf2990f5e14f1c9b9aae9a565791ed3eacb",encode_hex(&root[..32]),encode_hex(&root[96..128])],"data":encode_hex(log_data)}]});
    let transaction = json!({"to":anchor,"hash":transaction_hash,"blockHash":block_hash,"input":encode_hex(input)});
    let responses = vec![
        json!("0x7a69"),
        receipt,
        json!({"hash":block_hash}),
        transaction,
        json!("0x6000"),
        json!(trust.program_v_key),
        json!(trust.leaf_v_key_digest),
        json!(trust.aggregate_v_key_digest),
        json!(address_word(31)),
        json!(address_word(32)),
        json!("0x6000"),
        json!("0x4388a21c687fdd5f218d7e3d13190cac4c5355818d3605fd5fb811df468ee696"),
        json!(encode_hex(word(1))),
    ];
    (trust, responses)
}
fn with_rpc<T>(responses: Vec<Value>, action: impl FnOnce(&str) -> T) -> T {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    let worker = thread::spawn(move || {
        let mut index = 0;
        while !flag.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body=serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":responses.get(index).unwrap_or(&Value::Null)})).unwrap();
            index += 1;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",body.len()).unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    let result = action(&url);
    done.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    result
}

#[test]
fn rpc_verification_requires_external_chain_anchor_keys_registry_code_and_mined_log() {
    let (input, path) = common::fixture();
    let bundle = open_bundle(
        &input,
        17,
        &parse_fields("events[0],credential", 8).unwrap(),
        path,
    )
    .unwrap();
    let (trust, responses) = records(&bundle);
    assert!(with_rpc(responses.clone(), |url| verify_on_chain(
        &bundle, &trust, url
    ))
    .is_ok());
    let mut wrong = trust.clone();
    wrong.chain_id = 1;
    assert!(with_rpc(responses.clone(), |url| verify_on_chain(
        &bundle, &wrong, url
    ))
    .is_err());
    let mut wrong = trust.clone();
    wrong.anchor_address = address(99);
    assert!(with_rpc(responses.clone(), |url| verify_on_chain(
        &bundle, &wrong, url
    ))
    .is_err());
    let mut wrong = trust.clone();
    wrong.leaf_v_key_digest = encode_hex([0; 32]);
    assert!(with_rpc(responses.clone(), |url| verify_on_chain(
        &bundle, &wrong, url
    ))
    .is_err());
    let mut wrong = trust.clone();
    wrong.accepted_policies[0].certificate_root = encode_hex([0; 32]);
    assert!(with_rpc(responses.clone(), |url| verify_on_chain(
        &bundle, &wrong, url
    ))
    .is_err());
    for index in [5, 6, 7, 8, 9, 11, 12] {
        let mut bad = responses.clone();
        bad[index] = json!(encode_hex([0; 32]));
        assert!(
            with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err(),
            "RPC getter {index} was ignored"
        );
    }
    let mut bad = responses.clone();
    bad[10] = json!("0x6001");
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    let mut bad = responses.clone();
    bad[1]["status"] = json!("0x0");
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    let mut bad = responses.clone();
    bad[1]["logs"][0]["topics"][2] = json!(encode_hex([0; 32]));
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    let mut bad = responses.clone();
    bad[1]["logs"][0]["address"] = json!(address(99));
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    let mut bad = responses.clone();
    bad[2]["hash"] = json!(encode_hex([0; 32]));
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    let mut bad = responses.clone();
    bad[3]["input"] = json!("0x");
    assert!(with_rpc(bad, |url| verify_on_chain(&bundle, &trust, url)).is_err());
    assert!(verify_on_chain(&bundle, &trust, "file:///invalid").is_err());
}

#[test]
fn runnable_cli_opens_selected_fields_and_reports_only_prototype_measurement() {
    let (input, path) = common::fixture();
    let bundle = open_bundle(
        &input,
        17,
        &parse_fields("events[0],credential", 8).unwrap(),
        path.clone(),
    )
    .unwrap();
    let (trust, responses) = records(&bundle);
    let directory = std::env::temp_dir().join(format!(
        "disclosure-cli-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let witness = directory.join("private.bin");
    let authentication = directory.join("path.json");
    let output = directory.join("view.json");
    let expected = directory.join("trust.json");
    std::fs::write(&witness, input.encode().unwrap()).unwrap();
    std::fs::write(&authentication, serde_json::to_vec(&path).unwrap()).unwrap();
    std::fs::write(&expected, serde_json::to_vec(&trust).unwrap()).unwrap();
    let open = std::process::Command::new(env!("CARGO_BIN_EXE_view-open"))
        .args([
            "--lot",
            "17",
            "--preset",
            "certifier",
            "--witness",
            witness.to_str().unwrap(),
            "--aggregation-path",
            authentication.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        open.status.success(),
        "{}",
        String::from_utf8_lossy(&open.stderr)
    );
    let report: Value = serde_json::from_slice(&open.stdout).unwrap();
    assert_eq!(report["formal"], false);
    assert_eq!(report["selectedEvents"], 1);
    let opened: eudr_disclosure::Bundle =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(opened.events.len(), 1);
    assert!(opened.polygon.is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let verify = with_rpc(responses, |url| {
        std::process::Command::new(env!("CARGO_BIN_EXE_view-verify"))
            .args([
                "--bundle",
                output.to_str().unwrap(),
                "--trust",
                expected.to_str().unwrap(),
                "--rpc",
                url,
            ])
            .output()
            .unwrap()
    });
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let report: Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(report["formal"], false);
    assert_eq!(report["official30Repetitions"], "pending");
    assert_eq!(report["lotId"], 17);
    assert!(report["bundleBytes"].as_u64().unwrap() > 0);
    assert!(report["openingVerifyMicros"].is_number());
    std::fs::remove_dir_all(directory).unwrap();
}
