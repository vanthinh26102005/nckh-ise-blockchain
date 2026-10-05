use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn cli_writes_canonical_artifacts_and_refuses_existing_outputs() {
    let dir = std::env::temp_dir().join(format!(
        "epcis-mapper-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let json = dir.join("mapped.json");
    let bytes = dir.join("mapped.bin");
    let catalog = dir.join("catalog.json");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_epcis-mapper"))
            .arg(root.join("fixtures/object-event.json"))
            .arg(root.join("fixtures/mapping-context.json"))
            .arg("--out")
            .arg(&json)
            .arg("--bytes-out")
            .arg(&bytes)
            .arg("--catalog-out")
            .arg(&catalog)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let raw = fs::read(&bytes).unwrap();
    assert_eq!(raw.len(), 86);
    eudr_policy::decode_event(raw.as_slice().try_into().unwrap()).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&fs::read(&json).unwrap()).unwrap();
    assert_eq!(result["canonical_hex"].as_str().unwrap().len(), 172);
    assert!(result.get("signature_hex").is_none());
    let original = fs::read(&json).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read(&json).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}
