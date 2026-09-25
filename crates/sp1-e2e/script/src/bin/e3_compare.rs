use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use e2_bench::{accumulate_lots, generate_poisson_event_stream};
use ed25519_dalek::{Signer, SigningKey};
use eudr_epoch_policy::{leaf_nullifier_id, LeafInput};
use eudr_policy::{nullifier_default_root, nullifier_index, EpcisEventV1, SparseNullifierMap};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sp1_e2e::{
    benchmark_nullifier_secret,
    epoch::{prove_epoch_with, setup_epoch_provers},
    http::{FabricGateway, HttpEndpoint},
    policy_input_from_events, EventWitness,
};
use sp1_sdk::{
    blocking::{CpuProver, Prover, ProverClient},
    HashableKey, ProvingKey,
};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Baseline {
    FabricOnly,
    HashOnChain,
    VecroTokenAdapted,
    Proposed,
}

impl Baseline {
    fn name(self) -> &'static str {
        match self {
            Self::FabricOnly => "fabric-only",
            Self::HashOnChain => "hash-on-chain",
            Self::VecroTokenAdapted => "vecro-token-adapted",
            Self::Proposed => "proposed",
        }
    }
    fn uses_fabric(self) -> bool {
        matches!(self, Self::FabricOnly | Self::Proposed)
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProverKind {
    Cpu,
    Cuda,
}

#[derive(Parser)]
#[command(about = "E3 four-baseline runner; run each backend against the same seeded manifest")]
struct Cli {
    #[arg(long, value_enum)]
    baseline: Baseline,
    #[arg(long)]
    main: bool,
    #[arg(long, default_value_t = 480.0)]
    lambda_events_per_min: f64,
    #[arg(long, default_value_t = 1.0)]
    duration_min: f64,
    #[arg(long, default_value_t = 1)]
    seeds: u64,
    #[arg(long, default_value_t = 125)]
    producers: usize,
    #[arg(long, default_value_t = 30.0)]
    drain_min: f64,
    #[arg(long)]
    accelerated: bool,
    #[arg(long, default_value_t = 21)]
    run_id: u64,
    #[arg(long, default_value = "")]
    hardware_label: String,
    #[arg(long, value_enum, default_value_t = ProverKind::Cpu)]
    prover: ProverKind,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fabric_gateway: String,
    #[arg(long, default_value = "http://127.0.0.1:8547")]
    anvil_server: String,
    #[arg(long, default_value = "results/e3/raw/run.jsonl")]
    out: PathBuf,
}

#[derive(Clone)]
struct ManifestEvent {
    due_ms: u64,
    producer_id: usize,
    witness: EventWitness,
}

#[derive(Clone)]
struct Shipment {
    id: u64,
    epoch_id: u64,
    events: Vec<ManifestEvent>,
}

struct Produced {
    shipment: Shipment,
    ready_at: Instant,
    acknowledged_at: Instant,
    error: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate(&cli)?;
    if cli.baseline != Baseline::Proposed {
        return run::<CpuProver>(&cli, None);
    }
    match cli.prover {
        ProverKind::Cpu => run(&cli, Some(ProverClient::builder().cpu().build())),
        ProverKind::Cuda => run_cuda(&cli),
    }
}

#[cfg(feature = "cuda")]
fn run_cuda(cli: &Cli) -> Result<()> {
    run(cli, Some(ProverClient::builder().cuda().build()))
}

#[cfg(not(feature = "cuda"))]
fn run_cuda(_: &Cli) -> Result<()> {
    bail!("CUDA prover requires rebuilding sp1-e2e with --features cuda")
}

fn run<P: Prover>(cli: &Cli, prover: Option<P>) -> Result<()> {
    if let Some(parent) = cli.out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = File::create(&cli.out)?;
    let fabric = if cli.baseline.uses_fabric() {
        Some(FabricGateway::from_url(&cli.fabric_gateway)?)
    } else {
        None
    };
    let anvil = if cli.baseline == Baseline::FabricOnly {
        None
    } else {
        Some(HttpEndpoint::from_url(&cli.anvil_server)?)
    };
    let epoch = if let Some(prover) = prover {
        let keys = setup_epoch_provers(&prover).map_err(|e| anyhow!(e))?;
        Some((prover, keys))
    } else {
        None
    };
    let git_commit = command_output("git", &["rev-parse", "HEAD"]);
    let git_state = command_output("git", &["status", "--porcelain"]);
    for seed in 1..=cli.seeds {
        let shipments = manifest(&cli, seed)?;
        let digest = manifest_digest(&shipments);
        let reset = if let Some(server) = &anvil {
            let body = if let Some((_, (_, aggregate_key))) = &epoch {
                json!({ "programVKey": aggregate_key.verifying_key().bytes32(),
                    "initialNullifierRoot": format!("0x{}", hex::encode(nullifier_default_root())) })
            } else {
                json!({})
            };
            Some(call(server, "/reset", &body)?)
        } else {
            None
        };
        write(
            &mut out,
            &json!({
                "kind":"run", "baseline":cli.baseline.name(), "seed":seed,
                "runId":cli.run_id, "mode":if cli.accelerated {"accelerated-pilot"} else if cli.main {"main"} else {"paced-pilot"},
                "lambdaEventsPerMin":cli.lambda_events_per_min, "durationMin":cli.duration_min,
                "drainMin":cli.drain_min, "producerCount":cli.producers,
                "expectedShipments":shipments.len(), "manifestSha256":digest,
                "anvil":reset, "proofTiming":"uncached compressed leaf and Groth16 aggregate",
                "gitCommit":git_commit, "gitDirty":!git_state.is_empty(),
                "hardwareLabel":cli.hardware_label, "os":std::env::consts::OS,
                "arch":std::env::consts::ARCH, "sp1CircuitVersion":sp1_sdk::SP1_CIRCUIT_VERSION,
                "prover":format!("{:?}",cli.prover).to_lowercase(),
            "leafVKey":epoch.as_ref().map(|(_, (leaf, _))| leaf.verifying_key().bytes32()),
            "aggregateVKey":epoch.as_ref().map(|(_, (_, aggregate))| aggregate.verifying_key().bytes32()),
                "command":std::env::args().collect::<Vec<_>>(),
                "startedUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
            }),
        )?;
        let started = Instant::now();
        let (sender, receiver) = mpsc::channel();
        let producer = spawn_producer(shipments, fabric.clone(), cli.accelerated, started, sender);
        let mut map = SparseNullifierMap::default();
        let mut group: Vec<Produced> = Vec::new();
        for produced in receiver {
            if cli.baseline == Baseline::Proposed {
                if !group.is_empty() && group[0].shipment.epoch_id != produced.shipment.epoch_id {
                    process_group(
                        &cli,
                        seed,
                        &mut out,
                        started,
                        &mut map,
                        &epoch.as_ref().unwrap().0,
                        &epoch.as_ref().unwrap().1,
                        anvil.as_ref().unwrap(),
                        &group,
                    )?;
                    group.clear();
                }
                group.push(produced);
            } else {
                process_one(
                    &cli,
                    seed,
                    &mut out,
                    started,
                    fabric.as_ref(),
                    anvil.as_ref(),
                    produced,
                )?;
            }
        }
        if !group.is_empty() {
            process_group(
                &cli,
                seed,
                &mut out,
                started,
                &mut map,
                &epoch.as_ref().unwrap().0,
                &epoch.as_ref().unwrap().1,
                anvil.as_ref().unwrap(),
                &group,
            )?;
        }
        producer
            .join()
            .map_err(|_| anyhow!("producer thread panicked"))?;
        write(
            &mut out,
            &json!({"kind":"seed-end", "baseline":cli.baseline.name(), "seed":seed,
            "elapsedMs":started.elapsed().as_millis()}),
        )?;
        out.flush()?;
    }
    Ok(())
}

fn validate(cli: &Cli) -> Result<()> {
    if cli.baseline != Baseline::Proposed && matches!(cli.prover, ProverKind::Cuda) {
        bail!("--prover cuda is only valid for the Proposed baseline");
    }
    if cli.producers != 125
        || cli.run_id == 0
        || cli.seeds == 0
        || !cli.lambda_events_per_min.is_finite()
        || cli.lambda_events_per_min <= 0.0
        || !cli.duration_min.is_finite()
        || cli.duration_min <= 0.0
        || !cli.drain_min.is_finite()
        || cli.drain_min < 0.0
    {
        bail!("invalid E3 workload options");
    }
    if cli.main
        && (cli.accelerated
            || cli.seeds != 30
            || cli.duration_min != 60.0
            || cli.lambda_events_per_min != 480.0
            || cli.hardware_label.is_empty())
    {
        bail!("main run requires 480 event/min, 60 min, 30 seeds, and real pacing");
    }
    Ok(())
}

fn manifest(cli: &Cli, seed: u64) -> Result<Vec<Shipment>> {
    let stream = generate_poisson_event_stream(seed, cli.lambda_events_per_min, cli.duration_min);
    let lots = accumulate_lots(&stream, seed);
    let base = cli
        .run_id
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(seed.checked_mul(100_000)?))
        .ok_or_else(|| anyhow!("run/seed IDs overflow"))?;
    lots.into_iter()
        .enumerate()
        .map(|(index, lot)| {
            let id = base
                .checked_add(lot.lot_id as u64)
                .ok_or_else(|| anyhow!("shipment ID overflow"))?;
            let epoch_id = base
                .checked_add((index / 8 + 1) as u64)
                .ok_or_else(|| anyhow!("epoch ID overflow"))?;
            let events = lot
                .events
                .into_iter()
                .map(|source| {
                    let producer_id = (source.event_id - 1) % cli.producers;
                    let mut key_material = Sha256::new();
                    key_material.update(b"EUDR:E3:PRODUCER:V1");
                    key_material.update(seed.to_be_bytes());
                    key_material.update((producer_id as u64).to_be_bytes());
                    let key = SigningKey::from_bytes(&key_material.finalize().into());
                    let event = EpcisEventV1 {
                        event_id: base + source.event_id as u64,
                        lot_id: id,
                        epoch_id,
                        timestamp_ms: source.payload.timestamp_ms,
                        readings: source.payload.readings,
                        latitude_e6: source.payload.latitude_e6,
                        longitude_e6: source.payload.longitude_e6,
                        certificate_id: source.payload.certificate_id,
                        role: source.payload.role,
                        actor_public_key: key.verifying_key().to_bytes(),
                    };
                    let bytes = event.canonical_bytes();
                    ManifestEvent {
                        due_ms: source.ingest_at_ms,
                        producer_id,
                        witness: EventWitness {
                            event_bytes: bytes,
                            signature: key.sign(&bytes).to_bytes(),
                            certificate_path: [[0; 32]; eudr_policy::CERTIFICATE_TREE_DEPTH],
                        },
                    }
                })
                .collect();
            Ok(Shipment {
                id,
                epoch_id,
                events,
            })
        })
        .collect()
}

fn manifest_digest(shipments: &[Shipment]) -> String {
    let mut h = Sha256::new();
    h.update(b"EUDR:E3:MANIFEST:V1");
    for shipment in shipments {
        h.update(shipment.id.to_be_bytes());
        h.update(shipment.epoch_id.to_be_bytes());
        for event in &shipment.events {
            h.update(event.due_ms.to_be_bytes());
            h.update((event.producer_id as u64).to_be_bytes());
            h.update(event.witness.event_bytes);
            h.update(event.witness.signature);
        }
    }
    hex::encode(h.finalize())
}

fn shipment_digest(shipment: &Shipment) -> String {
    let mut h = Sha256::new();
    h.update(b"EUDR:E3:SHIPMENT:V1");
    h.update(shipment.id.to_be_bytes());
    h.update((shipment.events.len() as u32).to_be_bytes());
    for event in &shipment.events {
        h.update(event.witness.event_bytes);
    }
    format!("0x{}", hex::encode(h.finalize()))
}

fn spawn_producer(
    shipments: Vec<Shipment>,
    fabric: Option<FabricGateway>,
    accelerated: bool,
    started: Instant,
    sender: mpsc::Sender<Produced>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for shipment in shipments {
            let mut error = None;
            for event in &shipment.events {
                if !accelerated {
                    let due = started + Duration::from_millis(event.due_ms);
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        thread::sleep(wait);
                    }
                }
                if error.is_none() {
                    if let Some(gateway) = &fabric {
                        if let Err(e) = gateway.post_event(&event.witness.event_bytes) {
                            error = Some(e.to_string());
                        }
                    }
                }
            }
            let ready_at = started + Duration::from_millis(shipment.events.last().unwrap().due_ms);
            if sender
                .send(Produced {
                    shipment,
                    ready_at,
                    acknowledged_at: Instant::now(),
                    error,
                })
                .is_err()
            {
                break;
            }
        }
    })
}

fn process_one(
    cli: &Cli,
    seed: u64,
    out: &mut File,
    started: Instant,
    fabric: Option<&FabricGateway>,
    anvil: Option<&HttpEndpoint>,
    produced: Produced,
) -> Result<()> {
    let result = (|| -> Result<Value> {
        if let Some(error) = &produced.error {
            bail!(error.clone());
        }
        match cli.baseline {
            Baseline::FabricOnly => {
                for event in &produced.shipment.events {
                    fabric
                        .unwrap()
                        .assert_event_was_committed(&event.witness.event_bytes)?;
                }
                Ok(json!({"gasUsed":null, "calldataBytes":null, "transactionHash":null}))
            }
            Baseline::HashOnChain | Baseline::VecroTokenAdapted => {
                let endpoint = if cli.baseline == Baseline::HashOnChain {
                    "/hash"
                } else {
                    "/token"
                };
                call(
                    anvil.unwrap(),
                    endpoint,
                    &json!({"shipmentId":produced.shipment.id.to_string(),
                    "digest":shipment_digest(&produced.shipment)}),
                )
            }
            Baseline::Proposed => unreachable!(),
        }
    })();
    write_row(cli, seed, out, started, &produced, result, None)
}

fn process_group<P: Prover>(
    cli: &Cli,
    seed: u64,
    out: &mut File,
    started: Instant,
    map: &mut SparseNullifierMap,
    prover: &P,
    keys: &(P::ProvingKey, P::ProvingKey),
    anvil: &HttpEndpoint,
    group: &[Produced],
) -> Result<()> {
    let result = (|| -> Result<Value> {
        for item in group {
            if let Some(error) = &item.error {
                bail!("Fabric ingestion failed: {error}");
            }
        }
        let mut next_map = map.clone();
        let mut inputs = Vec::new();
        for item in group {
            let shipment = &item.shipment;
            let nullifier_id = leaf_nullifier_id(shipment.epoch_id, shipment.id, 0);
            let secret = benchmark_nullifier_secret(seed, nullifier_id);
            let index = nullifier_index(nullifier_id, secret);
            let path = next_map
                .empty_path(index)
                .map_err(|e| anyhow!("C5 path: {e:?}"))?;
            let policy = policy_input_from_events(
                shipment.epoch_id,
                shipment.id,
                shipment
                    .events
                    .iter()
                    .map(|event| event.witness.clone())
                    .collect(),
                next_map.root(),
                secret,
                path,
            )
            .map_err(|e| anyhow!(e))?;
            let leaf = LeafInput {
                shipment_id: shipment.id,
                leaf_index: 0,
                leaf_count: 1,
                policy,
            };
            leaf.evaluate().map_err(|e| anyhow!("leaf policy: {e:?}"))?;
            next_map
                .insert(index)
                .map_err(|e| anyhow!("C5 insert: {e:?}"))?;
            inputs.push(leaf);
        }
        let proving_started = Instant::now();
        let epoch = prove_epoch_with(prover, &keys.0, &keys.1, &inputs).map_err(|e| anyhow!(e))?;
        let proof_ms = proving_started.elapsed().as_millis();
        let groth16_proof_bytes = epoch.proof.bytes().len();
        if epoch.values.new_nullifier_root != next_map.root() {
            bail!("epoch root mismatch");
        }
        let fixture = sp1_e2e::epoch::evm_fixture(&epoch);
        let fixture: Value = serde_json::from_str(&fixture)?;
        let mut receipt = call(
            anvil,
            "/aggregate",
            &json!({
                "epochId":epoch.values.epoch_id.to_string(),
                "oldNullifierRoot":format!("0x{}", hex::encode(epoch.values.old_nullifier_root)),
                "newNullifierRoot":format!("0x{}", hex::encode(epoch.values.new_nullifier_root)),
                "epochCommitment":format!("0x{}", hex::encode(epoch.values.epoch_commitment)),
                "programVKey":epoch.program_vkey,
                "publicValues":fixture["publicValues"], "proof":fixture["proof"],
            }),
        )?;
        receipt["proofMs"] = json!(proof_ms);
        receipt["leafProvingMs"] = json!(epoch.leaf_proving_ms);
        receipt["aggregateProvingMs"] = json!(epoch.aggregate_proving_ms);
        receipt["groth16ProofBytes"] = json!(groth16_proof_bytes);
        *map = next_map;
        Ok(receipt)
    })();
    match result {
        Ok(receipt) => {
            let gas = receipt["gasUsed"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| anyhow!("aggregate receipt missing gas"))?;
            let bytes = receipt["calldataBytes"]
                .as_u64()
                .ok_or_else(|| anyhow!("aggregate receipt missing calldata"))?;
            for item in group {
                write_row(
                    cli,
                    seed,
                    out,
                    started,
                    item,
                    Ok(json!({"gasUsed":gas / group.len() as f64,
                        "calldataBytes":bytes as f64 / group.len() as f64,
                        "epochGasUsed":gas, "epochCalldataBytes":bytes,
                        "transactionHash":receipt["transactionHash"], "epochLotCount":group.len()})),
                    None,
                )?;
            }
        }
        Err(error) => {
            for item in group {
                write_row(
                    cli,
                    seed,
                    out,
                    started,
                    item,
                    Err(anyhow!(error.to_string())),
                    None,
                )?;
            }
        }
    }
    Ok(())
}

fn write_row(
    cli: &Cli,
    seed: u64,
    out: &mut File,
    started: Instant,
    produced: &Produced,
    result: Result<Value>,
    note: Option<&str>,
) -> Result<()> {
    let completed_at_ms = started.elapsed().as_millis();
    let ready_at_ms = produced
        .ready_at
        .saturating_duration_since(started)
        .as_millis();
    let acknowledged_at_ms = produced
        .acknowledged_at
        .saturating_duration_since(started)
        .as_millis();
    let deadline_ms = ((cli.duration_min + cli.drain_min) * 60_000.0) as u128;
    let status = if result.is_err() {
        "error"
    } else if !cli.accelerated && completed_at_ms > deadline_ms {
        "timeout"
    } else {
        "ok"
    };
    let (receipt, error) = match result {
        Ok(value) => (Some(value), None),
        Err(e) => (None, Some(e.to_string())),
    };
    write(
        out,
        &json!({
            "kind":"shipment", "baseline":cli.baseline.name(), "seed":seed,
            "shipmentId":produced.shipment.id.to_string(), "epochId":produced.shipment.epoch_id.to_string(),
            "eventCount":produced.shipment.events.len(), "manifestDigest":shipment_digest(&produced.shipment),
            "readyAtMs":ready_at_ms, "completedAtMs":completed_at_ms,
            "fabricAcknowledgedAtMs":if cli.baseline.uses_fabric() { Some(acknowledged_at_ms) } else { None },
            "auditLatencyMs":if receipt.is_some() { Some(completed_at_ms.saturating_sub(ready_at_ms)) } else { None },
            "status":status, "receipt":receipt, "error":error, "note":note,
        }),
    )
}

fn call(server: &HttpEndpoint, path: &str, body: &Value) -> Result<Value> {
    let response = server.request("POST", path, Some(&body.to_string()))?;
    if response.status != 201 {
        bail!("Anvil service {path}: {}", response.body);
    }
    serde_json::from_str(&response.body).context("parse Anvil service receipt")
}

fn write(out: &mut File, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn options() -> Cli {
        Cli {
            baseline: Baseline::FabricOnly,
            main: false,
            lambda_events_per_min: 480.0,
            duration_min: 1.0,
            seeds: 1,
            producers: 125,
            drain_min: 30.0,
            accelerated: true,
            run_id: 21,
            hardware_label: String::new(),
            prover: ProverKind::Cpu,
            fabric_gateway: String::new(),
            anvil_server: String::new(),
            out: PathBuf::new(),
        }
    }

    #[test]
    fn paired_manifest_is_deterministic_and_has_stable_producer_keys() {
        let cli = options();
        let first = manifest(&cli, 1).unwrap();
        let second = manifest(&cli, 1).unwrap();
        assert_eq!(manifest_digest(&first), manifest_digest(&second));
        let mut keys = HashMap::new();
        for shipment in &first {
            for event in &shipment.events {
                let key = &event.witness.event_bytes[54..86];
                match keys.insert(event.producer_id, key.to_vec()) {
                    Some(previous) => assert_eq!(previous.as_slice(), key),
                    None => {}
                }
                let decoded = eudr_policy::decode_event(&event.witness.event_bytes).unwrap();
                assert_eq!(decoded.lot_id, shipment.id);
                assert_eq!(decoded.epoch_id, shipment.epoch_id);
            }
        }
        assert_eq!(keys.len(), 125);
    }

    #[test]
    fn main_mode_cannot_be_accelerated() {
        let mut cli = options();
        cli.main = true;
        cli.seeds = 30;
        cli.duration_min = 60.0;
        cli.hardware_label = "test machine".into();
        assert!(validate(&cli).is_err());
        cli.accelerated = false;
        assert!(validate(&cli).is_ok());
    }
}
