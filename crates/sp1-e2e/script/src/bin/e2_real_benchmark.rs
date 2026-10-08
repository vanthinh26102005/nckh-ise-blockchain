use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use e2_bench::{accumulate_lots, generate_poisson_event_stream};
use eudr_policy::{evaluate, nullifier_default_root, nullifier_index, SparseNullifierMap};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sp1_e2e::{
    benchmark_batch_nonce, benchmark_events_for_lot, benchmark_nullifier_key, benchmark_owner,
    benchmark_registry,
    http::{FabricGateway, HttpEndpoint},
    policy_input_from_events, prove_evm_fixture_with, setup_epoch_prover, setup_policy_prover,
    EventWitness,
};
use sp1_sdk::{
    blocking::{Prover, ProverClient},
    HashableKey, ProvingKey,
};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "e2_real_benchmark/queue.rs"]
mod queue;

#[derive(Parser, Debug)]
#[command(about = "Real E2 Fabric acknowledgement-to-Anvil receipt benchmark")]
struct Cli {
    #[arg(long, default_value_t = 480.0)]
    lambda_events_per_min: f64,
    #[arg(long, default_value_t = 60.0)]
    duration_min: f64,
    #[arg(long, default_value_t = 30)]
    seeds: u64,
    /// Do not wait for Poisson timestamps. This is only for a debugging pilot, never paper data.
    #[arg(long)]
    accelerated: bool,
    /// CPU-only local acceptance with one seed and a hard 8–16 canonical-event cap.
    #[arg(long)]
    local_smoke: bool,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fabric_gateway: String,
    #[arg(long, default_value = "http://127.0.0.1:8546")]
    anchor_server: String,
    #[arg(long, value_enum, default_value_t = ProverKind::Cpu)]
    prover: ProverKind,
    /// New absolute output path outside this Git checkout; existing evidence is never overwritten.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Prebuild deterministic workload policy roots and program keys; setup only, never prove or post.
    #[arg(long)]
    prepare_policy_manifest: Option<PathBuf>,
    /// Read-only compute-host hardware/topology evidence from scripts/e2_preflight.py.
    #[arg(long)]
    preflight: Option<PathBuf>,
    #[arg(long, default_value_t = 8)]
    queue_capacity: usize,
    /// Drop and record an acknowledged lot if this bounded queue deadline expires.
    #[arg(long, default_value_t = 1.0)]
    queue_wait_timeout_sec: f64,
    /// Prevents duplicate Fabric event IDs when re-running an interrupted experiment.
    #[arg(long)]
    run_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProverKind {
    Cpu,
    Cuda,
}

struct ProducedLot {
    seed: u64,
    lot_id: u64,
    epoch_id: u64,
    events: Vec<EventWitness>,
    acknowledged_at: Instant,
}

struct PlannedLot {
    lot_id: u64,
    epoch_id: u64,
    events: Vec<EventWitness>,
    ingest_at_ms: Vec<u64>,
}

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeedCounters {
    scheduled_events: usize,
    scheduled_lots: usize,
    offered_events: usize,
    offered_lots: usize,
    committed_events: usize,
    committed_lots: usize,
    readback_events: usize,
    readback_lots: usize,
    queued_lots: usize,
    received_lots: usize,
    proved_lots: usize,
    proved_events: usize,
    anchored_lots: usize,
    anchored_events: usize,
    saturated_lots: usize,
    max_queue_depth: usize,
    producer_duration_sec: f64,
}

// Any consumer exit, including evidence write failure, cancels producer pacing.
struct StopProducerOnDrop(Arc<AtomicBool>);
impl Drop for StopProducerOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Default)]
struct ProducerOutcome {
    failures: BTreeMap<u64, (&'static str, String)>,
    committed_lots: HashSet<u64>,
    failure: Option<String>,
}

struct AnchorClient(HttpEndpoint);

impl AnchorClient {
    fn from_url(url: &str) -> Result<Self> {
        Ok(Self(HttpEndpoint::from_url(url)?))
    }

    fn reset(&self, initial_root: [u8; 32], program_vkey: &str) -> Result<()> {
        let body = json!({
            "initialNullifierRoot": bytes32(initial_root),
            "programVKey": program_vkey,
        })
        .to_string();
        let response = self.0.request("POST", "/reset", Some(&body))?;
        if response.status != 201 {
            bail!("Anvil anchor reset failed: {}", response.body);
        }
        Ok(())
    }

    fn anchor(&self, epoch_id: u64, old_root: [u8; 32], fixture: &str) -> Result<Value> {
        let fixture: Value =
            serde_json::from_str(fixture).context("parse locally verified SP1 fixture")?;
        let program_vkey = fixture["vkey"]
            .as_str()
            .ok_or_else(|| anyhow!("SP1 fixture has no vkey"))?;
        let public_values = fixture["publicValues"]
            .as_str()
            .ok_or_else(|| anyhow!("SP1 fixture has no publicValues"))?;
        let proof = fixture["proof"]
            .as_str()
            .ok_or_else(|| anyhow!("SP1 fixture has no proof"))?;
        let body = json!({
            "epochId": epoch_id.to_string(),
            "oldNullifierRoot": bytes32(old_root),
            "programVKey": program_vkey,
            "publicValues": public_values,
            "proof": proof,
        })
        .to_string();
        let response = self.0.request("POST", "/anchor", Some(&body))?;
        if response.status != 201 {
            bail!(
                "Anvil rejected the locally verified SP1 proof: {}",
                response.body
            );
        }
        serde_json::from_str(&response.body).context("parse Anvil anchor receipt")
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_cli(&cli)?;
    match cli.prover {
        ProverKind::Cpu => run(&cli, ProverClient::builder().cpu().build()),
        ProverKind::Cuda => run_cuda(&cli),
    }
}

#[cfg(feature = "cuda")]
fn run_cuda(cli: &Cli) -> Result<()> {
    run(cli, ProverClient::builder().cuda().build())
}

#[cfg(not(feature = "cuda"))]
fn run_cuda(_: &Cli) -> Result<()> {
    bail!(
        "CUDA prover is not compiled; rebuild with --features cuda inside the Slurm GPU allocation"
    )
}

fn run<P: Prover>(cli: &Cli, prover: P) -> Result<()> {
    if let Some(path) = &cli.prepare_policy_manifest {
        return prepare_policy_manifest(cli, &prover, path);
    }
    let raw_manifest = std::env::var("POLICY_MANIFEST")
        .context("POLICY_MANIFEST must name the trusted prebuilt policy manifest")
        .and_then(|path| fs::read(path).context("read trusted policy manifest"));
    let manifest_hint = raw_manifest
        .as_ref()
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
    let run_id = cli
        .run_id
        .or_else(|| {
            manifest_hint
                .as_ref()
                .and_then(|manifest| manifest["runId"].as_u64())
        })
        .unwrap_or_else(default_run_id);
    let output_path = cli
        .out
        .as_ref()
        .ok_or_else(|| anyhow!("--out is required for runtime"))?;
    let mut output = create_output(output_path)?;
    let (revision, dirty) = source_revision();
    let preflight_raw = cli
        .preflight
        .as_ref()
        .map(|path| fs::read(path).context("read E2 preflight"));
    let preflight = preflight_raw
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
    let compute_host = sp1_e2e::compute_host().map_err(|e|anyhow!(e))?;
    write_json_line(
        &mut output,
        &json!({
            "kind":"run", "protocolVersion":2, "policyVersion":2, "runId":run_id,
            "validationKind":if cli.local_smoke {"local-smoke"} else {"formal"},
            "sourceRevision":revision, "sourceDirty":dirty,
            "computeHost":compute_host,"sp1SdkVersion":sp1_sdk_version(),"preflight":preflight,
            "preflightFileSHA256":preflight_raw.as_ref().and_then(|result|result.as_ref().ok()).map(|bytes|hex::encode(Sha256::digest(bytes))),
            "preflightRecordSHA256":preflight.as_ref().and_then(|value|serde_json::to_vec(value).ok()).map(|bytes|hex::encode(Sha256::digest(bytes))),
            "registry_manifest_sha256":raw_manifest.as_ref().ok().map(|bytes|hex::encode(Sha256::digest(bytes))),
            "acceptedPolicies":manifest_hint.as_ref().map(|manifest|manifest["acceptedPolicies"].clone()),
            "lotIdentityDomain":"run_id * 1000000000 + seed * 1000000 + lot ordinal; fresh workload domains",
            "mode":if cli.accelerated {"accelerated-pilot"} else {"poisson-paced"},
            "prover":format!("{:?}",cli.prover).to_lowercase(),
            "lambdaEventsPerMin":cli.lambda_events_per_min, "durationMin":cli.duration_min, "seeds":cli.seeds,
            "queueCapacity":cli.queue_capacity,"queueWaitTimeoutSec":cli.queue_wait_timeout_sec,
            "queuePolicy":"bounded; record and drop saturated acknowledged lots, continue offered stream",
            "setupExcludedFromProofTiming":true,"registryBuiltBeforeProducer":true,
            "metric":"last Fabric commit acknowledgement to Anvil transaction receipt",
        }),
    )?;
    let setup = (|| -> Result<_> {
        if dirty {
            bail!("E2 requires a clean source checkout before Fabric events are posted");
        }
        if matches!(cli.prover, ProverKind::Cuda) && preflight.is_none() {
            bail!("CUDA E2 requires --preflight read-only hardware/topology evidence");
        }
        if let Some(configured) = &preflight {
            validate_preflight(configured, &revision, &compute_host, cli)?;
        }
        if preflight_raw.as_ref().is_some_and(|result| result.is_err())
            || (preflight_raw.is_some() && preflight.is_none())
        {
            bail!("E2 preflight is unreadable or invalid JSON");
        }
        let manifest: Value = serde_json::from_slice(
            raw_manifest
                .as_ref()
                .map_err(|error| anyhow!(error.to_string()))?,
        )?;
        let key = setup_policy_prover(&prover).map_err(|error| anyhow!(error))?;
        validate_policy_manifest(
            &manifest,
            &key.verifying_key().bytes32().to_string(),
            run_id,
            &revision,
            if cli.local_smoke { "local-smoke" } else { "formal" },
        )?;
        if manifest["lambdaEventsPerMin"].as_f64() != Some(cli.lambda_events_per_min)
            || manifest["durationMin"].as_f64() != Some(cli.duration_min)
            || manifest["seeds"].as_u64() != Some(cli.seeds)
        {
            bail!("runtime workload differs from trusted manifest preparation");
        }
        Ok((
            FabricGateway::from_url(&cli.fabric_gateway)?,
            AnchorClient::from_url(&cli.anchor_server)?,
            key,
            manifest,
        ))
    })();
    let (fabric, anchor, proving_key, trusted_manifest) = match setup {
        Ok(value) => value,
        Err(error) => {
            for seed in 1..=cli.seeds {
                write_seed_terminal(
                    &mut output,
                    run_id,
                    seed,
                    &SeedCounters::default(),
                    "failed",
                    Some(&error.to_string()),
                    0.0,
                )?;
            }
            write_json_line(
                &mut output,
                &json!({"kind":"summary","runId":run_id,"status":"failed","failureStage":"setup","error":error.to_string()}),
            )?;
            return Err(error);
        }
    };
    let program_vkey = proving_key.verifying_key().bytes32().to_string();
    write_json_line(
        &mut output,
        &json!({"kind":"setup","runId":run_id,"policyProgramVKey":program_vkey}),
    )?;
    let mut latencies = Vec::new();
    let mut incomplete_seeds = 0;
    for seed in 1..=cli.seeds {
        let seed_started = Instant::now();
        let schedules = lot_schedules(seed, cli.lambda_events_per_min, cli.duration_min);
        let schedule_hash = schedule_fingerprint(&schedules)?;
        let last_scheduled_at_ms = schedules
            .last()
            .and_then(|lot| lot.ingest_at_ms.last())
            .copied()
            .unwrap_or(0);
        let schedule_info: Vec<_> = schedules
            .iter()
            .map(|lot| (lot.lot_id, lot.ingest_at_ms.len()))
            .collect();
        let mut counters = SeedCounters {
            scheduled_events: schedules.iter().map(|lot| lot.ingest_at_ms.len()).sum(),
            scheduled_lots: schedules.len(),
            ..Default::default()
        };
        write_json_line(
            &mut output,
            &json!({"kind":"seed_plan","runId":run_id,"seed":seed,
            "scheduledEvents":counters.scheduled_events,"scheduledLots":counters.scheduled_lots,
            "scheduleSha256":schedule_hash,"lastScheduledAtMs":last_scheduled_at_ms}),
        )?;
        let planned = prepare_lots(schedules, seed, run_id);
        let planned = match planned {
            Ok(planned) => planned,
            Err(error) => {
                incomplete_seeds += 1;
                for (ordinal, event_count) in &schedule_info {
                    write_json_line(
                        &mut output,
                        &json!({"kind":"lot","runId":run_id,"seed":seed,
                        "lotId":global_lot_id(run_id,seed,*ordinal).ok(),"eventCount":event_count,
                        "status":"not_offered","failureStage":"planning","error":error.to_string()}),
                    )?;
                }
                write_seed_terminal(
                    &mut output,
                    run_id,
                    seed,
                    &counters,
                    "failed",
                    Some(&error.to_string()),
                    seed_started.elapsed().as_secs_f64(),
                )?;
                continue;
            }
        };
        let lot_info: Vec<_> = planned
            .iter()
            .map(|lot| (lot.lot_id, lot.epoch_id, lot.events.len()))
            .collect();
        let all_events: Vec<_> = planned
            .iter()
            .flat_map(|lot| lot.events.iter().cloned())
            .collect();
        // Issuer fixture creation is outside the measured producer/prover pipeline.
        let setup_seed = (|| -> Result<_> {
            let registry = benchmark_registry(seed, &all_events).map_err(|error| anyhow!(error))?;
            require_trusted_pair(&trusted_manifest, &registry)?;
            let configured = trusted_manifest["workloadSeeds"]
                .as_array()
                .and_then(|seeds| seeds.iter().find(|row| row["seed"].as_u64() == Some(seed)))
                .ok_or_else(|| anyhow!("trusted manifest omits this planned workload seed"))?;
            if configured["scheduledEvents"].as_u64() != Some(counters.scheduled_events as u64)
                || configured["scheduledLots"].as_u64() != Some(counters.scheduled_lots as u64)
                || configured["scheduleSha256"].as_str() != Some(schedule_hash.as_str())
            {
                bail!("runtime schedule differs from trusted prebuilt workload; no Fabric events posted");
            }
            anchor.reset(nullifier_default_root(), &program_vkey)?;
            Ok(registry)
        })();
        drop(all_events);
        let registry = match setup_seed {
            Ok(registry) => registry,
            Err(error) => {
                incomplete_seeds += 1;
                for (lot_id, epoch_id, event_count) in &lot_info {
                    write_json_line(
                        &mut output,
                        &json!({"kind":"lot","runId":run_id,"seed":seed,"lotId":lot_id,
                        "epochId":epoch_id,"eventCount":event_count,"status":"not_offered","failureStage":"seed_setup","error":error.to_string()}),
                    )?;
                }
                write_seed_terminal(
                    &mut output,
                    run_id,
                    seed,
                    &counters,
                    "failed",
                    Some(&error.to_string()),
                    seed_started.elapsed().as_secs_f64(),
                )?;
                continue;
            }
        };
        write_json_line(
            &mut output,
            &json!({"kind":"registry","runId":run_id,"seed":seed,"certificateRoot":bytes32(registry.root()),
                "polygonCommitment":bytes32(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce))}),
        )?;
        let shared = Arc::new(Mutex::new(counters.clone()));
        let stop = Arc::new(AtomicBool::new(false));
        let _stop_on_exit = StopProducerOnDrop(Arc::clone(&stop));
        let (sender, receiver) = mpsc::sync_channel(cli.queue_capacity);
        let producer = spawn_fabric_producer(
            fabric.clone(),
            sender,
            planned,
            seed,
            cli.accelerated,
            Duration::from_secs_f64(cli.queue_wait_timeout_sec),
            Arc::clone(&stop),
            Arc::clone(&shared),
            cli.queue_capacity,
        );
        let mut nullifiers = SparseNullifierMap::default();
        let mut recorded = HashSet::new();
        let mut failure = None;
        for lot in receiver.iter() {
            shared.lock().unwrap().received_lots += 1;
            let event_count = lot.events.len();
            let mut stage = "witness";
            let processed = (|| -> Result<Value> {
                let key = benchmark_nullifier_key(lot.seed, lot.lot_id);
                let index = nullifier_index(lot.lot_id, key);
                let old_root = nullifiers.root();
                let input = policy_input_from_events(
                    lot.epoch_id,
                    lot.lot_id,
                    lot.events,
                    old_root,
                    key,
                    nullifiers
                        .empty_path(index)
                        .map_err(|error| anyhow!("construct C5 witness: {error:?}"))?,
                    &registry,
                    benchmark_owner(seed),
                    benchmark_batch_nonce(seed, lot.lot_id),
                )
                .map_err(|error| anyhow!(error))?;
                let expected = evaluate(&input)
                    .map_err(|error| anyhow!("host policy pre-check: {error:?}"))?;
                stage = "prove";
                let proof_started = Instant::now();
                let fixture = prove_evm_fixture_with(&prover, &proving_key, &input)
                    .map_err(|error| anyhow!(error))?;
                let proof_bytes = fixture_proof_bytes(&fixture)?;
                let proof_ms = proof_started.elapsed().as_millis();
                {
                    let mut stats = shared.lock().unwrap();
                    stats.proved_lots += 1;
                    stats.proved_events += event_count;
                }
                stage = "anchor";
                let anchor_started = Instant::now();
                let receipt = anchor.anchor(lot.epoch_id, old_root, &fixture)?;
                let anchor_ms = anchor_started.elapsed().as_millis();
                if receipt["protocolVersion"].as_u64() != Some(2)
                    || receipt["verificationKind"].as_str() != Some("sp1-groth16")
                    || receipt["status"].as_u64() != Some(1)
                    || receipt["policyValid"].as_bool() != Some(true)
                    || receipt["certificateRoot"].as_str()
                        != Some(bytes32(expected.certificate_root).as_str())
                    || receipt["polygonCommitment"].as_str()
                        != Some(bytes32(expected.polygon_commitment).as_str())
                {
                    bail!("Anvil receipt is missing matching policy-v2 verification metadata");
                }
                if receipt["newNullifierRoot"].as_str()
                    != Some(bytes32(expected.new_nullifier_root).as_str())
                {
                    bail!("Anvil applied a root different from the locally verified proof");
                }
                let transaction_hash = receipt["transactionHash"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Anvil receipt lacks transaction hash"))?;
                if transaction_hash.len() != 66
                    || !transaction_hash.starts_with("0x")
                    || !transaction_hash[2..].bytes().all(|b| b.is_ascii_hexdigit())
                {
                    bail!("Anvil receipt has invalid transaction hash");
                }
                let gas_used = receipt["gasUsed"]
                    .as_str()
                    .filter(|gas| !gas.is_empty() && gas.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or_else(|| anyhow!("Anvil receipt lacks numeric gas usage"))?;
                nullifiers
                    .insert(index)
                    .map_err(|error| anyhow!("apply C5 transition: {error:?}"))?;
                if nullifiers.root() != expected.new_nullifier_root {
                    bail!("host nullifier map diverged from proved transition");
                }
                let latency = lot.acknowledged_at.elapsed().as_millis();
                {
                    let mut stats = shared.lock().unwrap();
                    stats.anchored_lots += 1;
                    stats.anchored_events += event_count;
                }
                latencies.push(latency as f64);
                Ok(
                    json!({"kind":"lot","runId":run_id,"seed":seed,"lotId":lot.lot_id,"epochId":lot.epoch_id,
                    "eventCount":event_count,"proofMs":proof_ms,"anvilAnchorMs":anchor_ms,
                    "fabricAcknowledgementToReceiptMs":latency,"proofBytes":proof_bytes,
                    "gasUsed":gas_used,"transactionHash":transaction_hash,"status":"ok",
                    "fabricReadback":true,
                    "protocolVersion":2,"verificationKind":"sp1-groth16","certificateRoot":receipt["certificateRoot"],
                    "polygonCommitment":receipt["polygonCommitment"],"policyValid":true,
                    "receipt":receipt}),
                )
            })();
            recorded.insert(lot.lot_id);
            match processed {
                Ok(row) => write_json_line(&mut output, &row)?,
                Err(error) => {
                    write_json_line(
                        &mut output,
                        &json!({"kind":"lot","runId":run_id,"seed":seed,"lotId":lot.lot_id,
                        "epochId":lot.epoch_id,"eventCount":event_count,"status":"failed","failureStage":stage,"error":error.to_string()}),
                    )?;
                    failure = Some(error.to_string());
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        // Receiver stays alive until producer has observed stop. Sending is nonblocking.
        stop.store(true, Ordering::Release);
        let outcome = match producer.join() {
            Ok(outcome) => outcome,
            Err(_) => ProducerOutcome {
                failure: Some("Fabric producer panicked".into()),
                ..Default::default()
            },
        };
        counters = shared.lock().unwrap().clone();
        if failure.is_none() {
            failure = outcome.failure.clone();
        }
        for (lot_id, epoch_id, event_count) in &lot_info {
            if recorded.contains(lot_id) {
                continue;
            }
            let (stage, error) = outcome.failures.get(lot_id).cloned().unwrap_or_else(|| {
                if outcome.committed_lots.contains(lot_id) {
                    (
                        "consumer_stopped",
                        "acknowledged lot was not completed".into(),
                    )
                } else {
                    (
                        "producer_stopped",
                        "scheduled lot was not fully committed".into(),
                    )
                }
            });
            write_json_line(
                &mut output,
                &json!({"kind":"lot","runId":run_id,"seed":seed,"lotId":lot_id,
                "epochId":epoch_id,"eventCount":event_count,
                "status":if outcome.committed_lots.contains(lot_id){"failed"}else{"not_offered"},
                "failureStage":stage,"error":error}),
            )?;
        }
        let complete = counters.scheduled_lots > 0
            && counters.anchored_lots == counters.scheduled_lots
            && counters.offered_events == counters.scheduled_events
            && counters.saturated_lots == 0
            && failure.is_none();
        if !complete {
            incomplete_seeds += 1;
        }
        write_seed_terminal(
            &mut output,
            run_id,
            seed,
            &counters,
            if complete {
                "complete"
            } else if failure.is_some() {
                "failed"
            } else {
                "partial"
            },
            failure.as_deref(),
            seed_started.elapsed().as_secs_f64(),
        )?;
    }
    write_json_line(
        &mut output,
        &json!({"kind":"summary","runId":run_id,"status":if incomplete_seeds==0{"complete"}else{"partial"},
        "incompleteSeeds":incomplete_seeds,"lots":latencies.len(),"latenciesConditionedOnAnchoredLots":true,
        "medianMs":percentile(&latencies,50.0),"iqrMs":[percentile(&latencies,25.0),percentile(&latencies,75.0)],
        "p95Ms":percentile(&latencies,95.0),"p99Ms":percentile(&latencies,99.0)}),
    )?;
    if incomplete_seeds > 0 {
        bail!(
            "{incomplete_seeds} seeds incomplete; terminal records retained in {}",
            output_path.display()
        );
    }
    Ok(())
}

fn prepare_lots(schedules: Vec<LotSchedule>, seed: u64, run_id: u64) -> Result<Vec<PlannedLot>> {
    let mut next = global_lot_id(run_id, seed, 0)?;
    if schedules
        .iter()
        .map(|lot| lot.ingest_at_ms.len())
        .sum::<usize>()
        >= 1_000_000
    {
        bail!("per-seed event namespace is exhausted; reduce workload");
    }
    let mut plans = Vec::with_capacity(schedules.len());
    for schedule in schedules {
        let lot_id = global_lot_id(run_id, seed, schedule.lot_id)?;
        let epoch_id = lot_id;
        let events =
            benchmark_events_for_lot(seed, epoch_id, lot_id, next, schedule.ingest_at_ms.len())
                .map_err(|error| anyhow!(error))?;
        next = next
            .checked_add(events.len() as u64)
            .ok_or_else(|| anyhow!("event ID overflow"))?;
        plans.push(PlannedLot {
            lot_id,
            epoch_id,
            events,
            ingest_at_ms: schedule.ingest_at_ms,
        });
    }
    Ok(plans)
}

fn spawn_fabric_producer(
    fabric: FabricGateway,
    sender: mpsc::SyncSender<ProducedLot>,
    plans: Vec<PlannedLot>,
    seed: u64,
    accelerated: bool,
    queue_timeout: Duration,
    stop: Arc<AtomicBool>,
    counters: Arc<Mutex<SeedCounters>>,
    queue_capacity: usize,
) -> thread::JoinHandle<ProducerOutcome> {
    thread::spawn(move || {
        let started = Instant::now();
        let mut outcome = ProducerOutcome::default();
        'lots: for mut plan in plans {
            if stop.load(Ordering::Acquire) {
                break;
            }
            counters.lock().unwrap().offered_lots += 1;
            for (event, due_ms) in plan.events.iter().zip(&plan.ingest_at_ms) {
                if !accelerated {
                    let due = started + Duration::from_millis(*due_ms);
                    while let Some(wait) = due.checked_duration_since(Instant::now()) {
                        if stop.load(Ordering::Acquire) {
                            break 'lots;
                        }
                        thread::sleep(wait.min(Duration::from_millis(50)));
                    }
                }
                if stop.load(Ordering::Acquire) {
                    break 'lots;
                }
                counters.lock().unwrap().offered_events += 1;
                if let Err(error) = fabric.post_event(&event.event_bytes) {
                    outcome
                        .failures
                        .insert(plan.lot_id, ("fabric", error.to_string()));
                    outcome.failure = Some(error.to_string());
                    break 'lots;
                }
                counters.lock().unwrap().committed_events += 1;
            }
            counters.lock().unwrap().committed_lots += 1;
            outcome.committed_lots.insert(plan.lot_id);
            let acknowledged_at = Instant::now();
            for event in &mut plan.events {
                match fabric.read_committed_event(&event.event_bytes) {
                    Ok(ledger_bytes) => {
                        event.event_bytes = ledger_bytes;
                        counters.lock().unwrap().readback_events += 1;
                    }
                    Err(error) => {
                        outcome.failures.insert(plan.lot_id, ("fabric_readback", error.to_string()));
                        outcome.failure = Some(error.to_string());
                        break 'lots;
                    }
                }
            }
            counters.lock().unwrap().readback_lots += 1;
            let lot = ProducedLot {
                seed,
                lot_id: plan.lot_id,
                epoch_id: plan.epoch_id,
                events: plan.events,
                acknowledged_at,
            };
            match queue::send_before_deadline(&sender, lot, &stop, queue_timeout) {
                Ok(()) => {
                    let mut stats = counters.lock().unwrap();
                    stats.queued_lots += 1;
                    let depth = stats
                        .queued_lots
                        .saturating_sub(stats.received_lots)
                        .min(queue_capacity);
                    stats.max_queue_depth = stats.max_queue_depth.max(depth);
                }
                Err((lot, queue::SendFailure::Saturated)) => {
                    let mut stats = counters.lock().unwrap();
                    stats.saturated_lots += 1;
                    stats.max_queue_depth = queue_capacity;
                    outcome.failures.insert(
                        lot.lot_id,
                        ("queue_saturated", "bounded queue deadline expired".into()),
                    );
                }
                Err((lot, reason)) => {
                    outcome.failures.insert(
                        lot.lot_id,
                        ("queue_stopped", format!("consumer stopped: {reason:?}")),
                    );
                    break;
                }
            }
        }
        counters.lock().unwrap().producer_duration_sec = started.elapsed().as_secs_f64();
        outcome
    })
}

fn write_seed_terminal(
    output: &mut File,
    run_id: u64,
    seed: u64,
    counters: &SeedCounters,
    status: &str,
    failure: Option<&str>,
    duration: f64,
) -> Result<()> {
    let mut row = serde_json::to_value(counters)?;
    row["kind"] = json!("seed_terminal");
    row["runId"] = json!(run_id);
    row["seed"] = json!(seed);
    row["status"] = json!(status);
    row["failure"] = json!(failure);
    row["durationSec"] = json!(duration);
    write_json_line(output, &row)
}

fn source_revision() -> (String, bool) {
    let root = env!("CARGO_MANIFEST_DIR");
    let rev = sp1_e2e::source_revision();
    let dirty = std::process::Command::new("git")
        .args(["-C", root, "status", "--porcelain"])
        .output()
        .ok()
        .map(|out| {
            if out.status.success() {
                !out.stdout.is_empty()
            } else {
                std::env::var("SOURCE_REVISION").ok().as_deref() != Some(rev.as_str())
            }
        })
        .unwrap_or(true);
    (rev, dirty)
}


fn sp1_sdk_version() -> String {
    include_str!("../../../Cargo.lock")
        .split("[[package]]")
        .find(|entry| entry.lines().any(|line| line == "name = \"sp1-sdk\""))
        .and_then(|entry| {
            entry.lines().find_map(|line| {
                line.strip_prefix("version = \"")
                    .and_then(|value| value.strip_suffix('"'))
            })
        })
        .unwrap_or("unavailable")
        .to_string()
}

fn validate_preflight(record: &Value, revision: &str, host: &str, cli: &Cli) -> Result<()> {
    if record["kind"].as_str() != Some("e2_preflight")
        || record["protocolVersion"].as_u64() != Some(2)
        || record["sourceRevision"].as_str() != Some(revision)
        || record["computeHost"].as_str() != Some(host)
        || record["hardware"]["hostname"].as_str() != Some(host)
        || record["software"]["sp1_version"].as_str() != Some(sp1_sdk_version().as_str())
        || record["fabricGateway"].as_str() != Some(cli.fabric_gateway.as_str())
        || record["anchorServer"].as_str() != Some(cli.anchor_server.as_str())
    {
        bail!("E2 preflight source/host/SP1/endpoints differ from this binary and runtime");
    }
    let expected_kind = if cli.local_smoke { "local-smoke" } else { "formal" };
    if record["validationKind"].as_str() != Some(expected_kind) {
        bail!("E2 preflight validation kind differs from this run");
    }
    if cli.local_smoke {
        if record["prover"].as_str() != Some("cpu")
            || record["hardware"]["gpu_model_driver"].as_str() != Some("N/A")
            || record["hardware"]["cuda_toolkit"].as_str() != Some("N/A")
            || record["gpuUnavailableReason"]
                .as_str()
                .is_none_or(|reason| reason.is_empty())
        {
            bail!("local E2 needs CPU provenance and an explicit GPU/CUDA N/A reason");
        }
    }
    for key in [
        "cpu_model",
        "cpu_logical",
        "ram_bytes",
        "gpu_model_driver",
        "cuda_toolkit",
        "os",
    ] {
        if record["hardware"][key].is_null() || record["hardware"][key].as_str() == Some("") {
            bail!("E2 preflight is missing hardware {key}");
        }
    }
    for key in ["rustc", "cargo"] {
        if record["software"][key]
            .as_str()
            .is_none_or(|value| value.is_empty())
        {
            bail!("E2 preflight is missing toolchain {key}");
        }
    }
    let expected = [
        "e2-orderer1.example.com",
        "e2-orderer2.example.com",
        "e2-orderer3.example.com",
        "e2-peer0.org1.example.com",
        "e2-peer0.org2.example.com",
    ];
    let containers = record["topology"]["fabricContainers"]
        .as_object()
        .ok_or_else(|| anyhow!("E2 preflight omits observed Fabric topology"))?;
    if containers.len() != expected.len()
        || expected.iter().any(|name| !containers.contains_key(*name))
        || record["topology"]["anvilChainId"].as_str() != Some("0x7a69")
        || record["topology"]["anvilClientVersion"]
            .as_str()
            .is_none_or(|value| value.is_empty())
    {
        bail!("E2 preflight topology differs from 2org/2peer/3Raft and Anvil31337");
    }
    for container in containers.values() {
        for key in ["id", "image"] {
            if container[key].as_str().is_none_or(|value| value.is_empty()) {
                bail!("E2 preflight omits container/image provenance");
            }
        }
    }
    Ok(())
}

fn global_lot_id(run_id: u64, seed: u64, ordinal: u64) -> Result<u64> {
    if seed > 999 || ordinal >= 1_000_000 {
        bail!("seed/lot namespace exhausted");
    }
    run_id
        .checked_mul(1_000_000_000)
        .and_then(|v| {
            seed.checked_mul(1_000_000)
                .and_then(|offset| v.checked_add(offset))
        })
        .and_then(|v| v.checked_add(ordinal))
        .ok_or_else(|| anyhow!("run/seed/lot ID overflow"))
}

fn schedule_fingerprint(schedules: &[LotSchedule]) -> Result<String> {
    let schedule: Vec<_> = schedules
        .iter()
        .map(|lot| json!({"ordinal":lot.lot_id,"ingestAtMs":lot.ingest_at_ms}))
        .collect();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&schedule)?)))
}

fn validate_policy_manifest(
    manifest: &Value,
    program_key: &str,
    run_id: u64,
    revision: &str,
    validation_kind: &str,
) -> Result<()> {
    if manifest["protocolVersion"].as_u64() != Some(2)
        || manifest["runId"].as_u64() != Some(run_id)
        || manifest["sourceRevision"].as_str() != Some(revision)
        || manifest["sourceDirty"].as_bool() != Some(false)
        || manifest["validationKind"].as_str() != Some(validation_kind)
        || manifest["leafProgramVKey"]
            .as_str()
            .or_else(|| manifest["programVKey"].as_str())
            != Some(program_key)
    {
        bail!("trusted manifest must match policy-v2, clean source revision, validation kind, run ID and pinned leaf program key");
    }
    let pairs = manifest["acceptedPolicies"]
        .as_array()
        .filter(|pairs| !pairs.is_empty())
        .ok_or_else(|| anyhow!("trusted acceptedPolicies must be nonempty"))?;
    for pair in pairs {
        for field in ["certificateRoot", "polygonCommitment"] {
            let raw = pair[field]
                .as_str()
                .and_then(|raw| raw.strip_prefix("0x"))
                .ok_or_else(|| anyhow!("trusted policy root must be 0x-prefixed hex"))?;
            if raw.len() != 64 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("trusted policy root must be 32-byte hex");
            }
        }
    }
    Ok(())
}

fn require_trusted_pair(
    manifest: &Value,
    registry: &eudr_policy::registry::RegistrySnapshot,
) -> Result<()> {
    let pair = json!({"certificateRoot":bytes32(registry.root()),"polygonCommitment":bytes32(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce))});
    if !manifest["acceptedPolicies"]
        .as_array()
        .is_some_and(|pairs| pairs.contains(&pair))
    {
        bail!("prebuilt workload policy root pair is absent from POLICY_MANIFEST; no Fabric events posted");
    }
    Ok(())
}

fn prepare_policy_manifest<P: Prover>(cli: &Cli, prover: &P, path: &PathBuf) -> Result<()> {
    let run_id = cli.run_id.unwrap_or_else(default_run_id);
    let (revision, dirty) = source_revision();
    if dirty {
        bail!("E2 trusted manifest preparation requires a clean source checkout");
    }
    // Setup returns program keys only; neither prove nor execute nor RPC is used.
    let leaf = setup_policy_prover(prover).map_err(|error| anyhow!(error))?;
    let aggregate = setup_epoch_prover(prover).map_err(|error| anyhow!(error))?;
    let mut pairs = Vec::new();
    let mut workload_seeds = Vec::new();
    for seed in 1..=cli.seeds {
        let schedules = lot_schedules(seed, cli.lambda_events_per_min, cli.duration_min);
        let hash = schedule_fingerprint(&schedules)?;
        let last = schedules
            .last()
            .and_then(|lot| lot.ingest_at_ms.last())
            .copied()
            .unwrap_or(0);
        let plans = prepare_lots(schedules, seed, run_id)?;
        let events: Vec<_> = plans
            .iter()
            .flat_map(|lot| lot.events.iter().cloned())
            .collect();
        let registry = benchmark_registry(seed, &events).map_err(|error| anyhow!(error))?;
        let pair = json!({"certificateRoot":bytes32(registry.root()),"polygonCommitment":bytes32(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce))});
        workload_seeds.push(
            json!({"seed":seed,"scheduledEvents":events.len(),"scheduledLots":plans.len(),
            "scheduleSha256":hash,"lastScheduledAtMs":last,"policy":pair}),
        );
        pairs.push(pair);
    }
    let manifest = json!({"protocolVersion":2,"runId":run_id,"sourceRevision":revision,"sourceDirty":dirty,
        "validationKind":if cli.local_smoke {"local-smoke"} else {"formal"},
        "leafProgramVKey":leaf.verifying_key().bytes32().to_string(),"aggregateProgramVKey":aggregate.verifying_key().bytes32().to_string(),
        "leafVKeyDigest":bytes32(leaf.verifying_key().hash_bytes()),"aggregateVKeyDigest":bytes32(aggregate.verifying_key().hash_bytes()),
        "lambdaEventsPerMin":cli.lambda_events_per_min,"durationMin":cli.duration_min,"seeds":cli.seeds,
        "acceptedPolicies":pairs,"workloadSeeds":workload_seeds,"preparation":"deterministic synthetic issuer fixture; setup only; no proofs or network posts"});
    let mut file = create_output(path)?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    println!(
        "Prepared trusted policy manifest: {} (run ID {run_id})",
        path.display()
    );
    Ok(())
}

struct LotSchedule {
    lot_id: u64,
    ingest_at_ms: Vec<u64>,
}

fn lot_schedules(seed: u64, lambda_events_per_min: f64, duration_min: f64) -> Vec<LotSchedule> {
    let events = generate_poisson_event_stream(seed, lambda_events_per_min, duration_min);
    accumulate_lots(&events, seed)
        .into_iter()
        .map(|lot| LotSchedule {
            lot_id: lot.lot_id as u64,
            ingest_at_ms: lot
                .events
                .into_iter()
                .map(|event| event.ingest_at_ms)
                .collect(),
        })
        .collect()
}

fn validate_cli(cli: &Cli) -> Result<()> {
    if !cli.lambda_events_per_min.is_finite() || cli.lambda_events_per_min <= 0.0 {
        bail!("--lambda-events-per-min must be positive");
    }
    if !cli.duration_min.is_finite() || cli.duration_min <= 0.0 || cli.seeds == 0 {
        bail!("--duration-min and --seeds must be positive");
    }
    if cli.seeds > 999
        || cli.queue_capacity == 0
        || cli.queue_capacity > 65_536
        || !cli.queue_wait_timeout_sec.is_finite()
        || !(0.0..=3600.0).contains(&cli.queue_wait_timeout_sec)
    {
        bail!(
            "seeds must be 1..999, queue capacity 1..65536, queue timeout finite 0..3600 seconds"
        );
    }
    if cli.local_smoke {
        if !matches!(cli.prover, ProverKind::Cpu)
            || cli.seeds != 1
            || !cli.accelerated
            || cli.queue_capacity > 2
            || cli.queue_wait_timeout_sec > 10.0
        {
            bail!("local E2 requires CPU, --accelerated, one seed, queue capacity <=2 and queue timeout <=10 seconds");
        }
        validate_local_workload(cli.lambda_events_per_min, cli.duration_min)?;
        if cli.out.is_some() && cli.preflight.is_none() {
            bail!("local E2 runtime requires --preflight from the observed private stack");
        }
    }
    let destination = match (&cli.out, &cli.prepare_policy_manifest) {
        (Some(path), None) | (None, Some(path)) => path,
        _ => bail!(
            "provide exactly one of --out (runtime) or --prepare-policy-manifest (setup only)"
        ),
    };
    if !destination.is_absolute()
        || destination
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        bail!("--out must be a new absolute path without .. outside the Git checkout");
    }
    Ok(())
}

fn validate_local_workload(lambda_events_per_min: f64, duration_min: f64) -> Result<()> {
    if lambda_events_per_min * duration_min > 32.0 {
        bail!("local E2 expected event volume exceeds bounded preparation budget");
    }
    let events = generate_poisson_event_stream(1, lambda_events_per_min, duration_min);
    if !(8..=16).contains(&events.len()) {
        bail!("local E2 canonical event cap requires 8..16 events; schedule has {}", events.len());
    }
    let lots = accumulate_lots(&events, 1);
    if lots.is_empty() || lots.iter().map(|lot| lot.events.len()).sum::<usize>() != events.len() {
        bail!("local E2 needs at least one complete lot and exact event accounting");
    }
    Ok(())
}

fn create_output(path: &PathBuf) -> Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("output needs a parent directory"))?;
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| anyhow!("cannot locate Git checkout"))?
        .canonicalize()?;
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| anyhow!("output has no existing parent"))?;
    }
    if ancestor.canonicalize()?.starts_with(&checkout) {
        bail!("output must be outside the Git checkout");
    }
    fs::create_dir_all(parent)?;
    if parent.canonicalize()?.starts_with(&checkout) {
        bail!("output resolves inside the Git checkout");
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create new {}", path.display()))
}

fn write_json_line(output: &mut File, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn bytes32(value: [u8; 32]) -> String {
    format!("0x{}", hex::encode(value))
}

fn fixture_proof_bytes(fixture: &str) -> Result<usize> {
    let fixture: Value = serde_json::from_str(fixture)?;
    let proof = fixture["proof"]
        .as_str()
        .ok_or_else(|| anyhow!("SP1 fixture has no proof"))?;
    let hex = proof.strip_prefix("0x").unwrap_or(proof);
    if hex.len() % 2 != 0 {
        bail!("SP1 fixture has odd-length proof bytes");
    }
    Ok(hex.len() / 2)
}

fn percentile(values: &[f64], percentage: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = (percentage / 100.0) * (sorted.len() - 1) as f64;
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
}

fn default_run_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after Unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_schedule_has_only_provable_lots() {
        let schedules = lot_schedules(7, 480.0, 1.0);
        assert!(!schedules.is_empty());
        assert!(schedules
            .iter()
            .all(|lot| (8..=64).contains(&lot.ingest_at_ms.len())));
    }

    #[test]
    fn percentile_interpolates_between_samples() {
        assert_eq!(percentile(&[10.0, 20.0, 30.0, 40.0], 50.0), 25.0);
    }

    #[test]
    fn local_workload_has_a_hard_cap_on_real_canonical_events() {
        assert!(validate_local_workload(16.0, 1.0).is_ok());
        assert!(validate_local_workload(480.0, 1.0).is_err());
        assert!(validate_local_workload(1.0, 1.0).is_err());
    }

    #[test]
    fn trusted_manifest_rejects_revision_and_validation_kind_mismatch() {
        let key = format!("0x{}", "33".repeat(32));
        let mut manifest = json!({
            "protocolVersion":2,"runId":900002,"sourceRevision":"a".repeat(40),
            "sourceDirty":false,"validationKind":"local-smoke","leafProgramVKey":key,
            "acceptedPolicies":[{"certificateRoot":format!("0x{}", "11".repeat(32)),
                                  "polygonCommitment":format!("0x{}", "22".repeat(32))}]
        });
        assert!(validate_policy_manifest(&manifest, &key, 900002, &"a".repeat(40), "local-smoke").is_ok());
        assert!(validate_policy_manifest(&manifest, &key, 900002, &"b".repeat(40), "local-smoke").is_err());
        assert!(validate_policy_manifest(&manifest, &key, 900002, &"a".repeat(40), "formal").is_err());
        manifest["sourceDirty"] = json!(true);
        assert!(validate_policy_manifest(&manifest, &key, 900002, &"a".repeat(40), "local-smoke").is_err());
    }
}
