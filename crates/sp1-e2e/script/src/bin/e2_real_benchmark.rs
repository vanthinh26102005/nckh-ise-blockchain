use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use e2_bench::{accumulate_lots, generate_poisson_event_stream};
use eudr_policy::{evaluate, nullifier_default_root, nullifier_index, SparseNullifierMap};
use serde_json::{json, Value};
use sp1_e2e::{
    benchmark_events_for_lot, benchmark_nullifier_secret,
    http::{FabricGateway, HttpEndpoint},
    policy_input_from_events, prove_evm_fixture_with, setup_policy_prover, EventWitness,
};
use sp1_sdk::{
    blocking::{Prover, ProverClient},
    HashableKey, ProvingKey,
};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Parser, Debug)]
#[command(about = "Real E2 Fabric acknowledgement-to-Anvil receipt benchmark")]
struct Cli {
    #[arg(long, default_value_t = 480.0)]
    lambda_events_per_min: f64,
    #[arg(long, default_value_t = 1.0)]
    duration_min: f64,
    #[arg(long, default_value_t = 1)]
    seeds: u64,
    /// Do not wait for Poisson timestamps. This is only for a debugging pilot, never paper data.
    #[arg(long)]
    accelerated: bool,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fabric_gateway: String,
    #[arg(long, default_value = "http://127.0.0.1:8546")]
    anchor_server: String,
    #[arg(long, value_enum, default_value_t = ProverKind::Cpu)]
    prover: ProverKind,
    #[arg(long, default_value = "results/e2/real/raw.jsonl")]
    out: PathBuf,
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
    let fabric = FabricGateway::from_url(&cli.fabric_gateway)?;
    let anchor = AnchorClient::from_url(&cli.anchor_server)?;
    let proving_key = setup_policy_prover(&prover).map_err(|error| anyhow!(error))?;
    let program_vkey = proving_key.verifying_key().bytes32().to_string();
    let run_id = cli.run_id.unwrap_or_else(default_run_id);
    let mut output = create_output(&cli.out)?;
    write_json_line(
        &mut output,
        &json!({
            "kind": "run",
            "runId": run_id,
            "mode": if cli.accelerated { "accelerated-pilot" } else { "poisson-paced" },
            "prover": format!("{:?}", cli.prover).to_lowercase(),
            "lambdaEventsPerMin": cli.lambda_events_per_min,
            "durationMin": cli.duration_min,
            "seeds": cli.seeds,
            "setupExcludedFromProofTiming": true,
            "proofTiming": "each lot is independently proved and locally verified without a timing cache",
            "metric": "last Fabric commit acknowledgement to Anvil transaction receipt",
            "policyProgramVKey": program_vkey,
        }),
    )?;

    let mut latencies = Vec::new();
    for seed in 1..=cli.seeds {
        anchor.reset(nullifier_default_root(), &program_vkey)?;
        let schedules = lot_schedules(seed, cli.lambda_events_per_min, cli.duration_min);
        // ponytail: unbounded queue preserves ingest pacing; use back-pressure plus queue-depth
        // metrics if prover saturation itself becomes an experiment variable.
        let (sender, receiver) = mpsc::channel();
        let producer = spawn_fabric_producer(
            fabric.clone(),
            sender,
            schedules,
            seed,
            run_id,
            cli.accelerated,
        );
        let mut nullifiers = SparseNullifierMap::default();

        for lot in receiver {
            let secret = benchmark_nullifier_secret(lot.seed, lot.lot_id);
            let index = nullifier_index(lot.lot_id, secret);
            let old_root = nullifiers.root();
            let input = policy_input_from_events(
                lot.epoch_id,
                lot.lot_id,
                lot.events,
                old_root,
                secret,
                nullifiers
                    .empty_path(index)
                    .map_err(|error| anyhow!("construct C5 witness: {error:?}"))?,
            )
            .map_err(|error| anyhow!(error))?;
            let expected =
                evaluate(&input).map_err(|error| anyhow!("host policy pre-check: {error:?}"))?;

            let proof_started = Instant::now();
            let fixture = prove_evm_fixture_with(&prover, &proving_key, &input)
                .map_err(|error| anyhow!(error))?;
            let proof_ms = proof_started.elapsed().as_millis();
            let anchor_started = Instant::now();
            let receipt = anchor.anchor(lot.epoch_id, old_root, &fixture)?;
            let anchor_ms = anchor_started.elapsed().as_millis();
            let new_root = receipt["newNullifierRoot"]
                .as_str()
                .ok_or_else(|| anyhow!("Anvil receipt has no newNullifierRoot"))?;
            if new_root != bytes32(expected.new_nullifier_root) {
                bail!("Anvil applied a root that differs from the locally verified proof");
            }
            nullifiers
                .insert(index)
                .map_err(|error| anyhow!("apply C5 transition: {error:?}"))?;
            if nullifiers.root() != expected.new_nullifier_root {
                bail!("host nullifier map diverged from the proved transition");
            }

            let end_to_end_ms = lot.acknowledged_at.elapsed().as_millis();
            latencies.push(end_to_end_ms as f64);
            write_json_line(
                &mut output,
                &json!({
                    "kind": "lot",
                    "runId": run_id,
                    "seed": lot.seed,
                    "lotId": lot.lot_id,
                    "epochId": lot.epoch_id,
                    "eventCount": input.events.len(),
                    "proofMs": proof_ms,
                    "anvilAnchorMs": anchor_ms,
                    "fabricAcknowledgementToReceiptMs": end_to_end_ms,
                    "proofBytes": fixture_proof_bytes(&fixture)?,
                    "gasUsed": receipt["gasUsed"].as_str(),
                    "transactionHash": receipt["transactionHash"].as_str(),
                    "status": "ok",
                }),
            )?;
        }
        producer
            .join()
            .map_err(|_| anyhow!("Fabric producer thread panicked"))??;
    }

    write_json_line(
        &mut output,
        &json!({
            "kind": "summary",
            "lots": latencies.len(),
            "medianMs": percentile(&latencies, 50.0),
            "iqrMs": [percentile(&latencies, 25.0), percentile(&latencies, 75.0)],
            "p95Ms": percentile(&latencies, 95.0),
        }),
    )?;
    Ok(())
}

fn spawn_fabric_producer(
    fabric: FabricGateway,
    sender: mpsc::Sender<ProducedLot>,
    schedules: Vec<LotSchedule>,
    seed: u64,
    run_id: u64,
    accelerated: bool,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let started = Instant::now();
        let mut next_event_id = run_id
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_add(seed * 100_000))
            .ok_or_else(|| anyhow!("run ID is too large for canonical event IDs"))?;
        for schedule in schedules {
            let epoch_id = seed * 1_000_000 + schedule.lot_id;
            let events = benchmark_events_for_lot(
                seed,
                epoch_id,
                schedule.lot_id,
                next_event_id,
                schedule.ingest_at_ms.len(),
            )
            .map_err(|error| anyhow!(error))?;
            next_event_id += events.len() as u64;
            for (witness, due_ms) in events.iter().zip(&schedule.ingest_at_ms) {
                if !accelerated {
                    let due = started + Duration::from_millis(*due_ms);
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        thread::sleep(wait);
                    }
                }
                fabric.post_event(&witness.event_bytes)?;
            }
            sender
                .send(ProducedLot {
                    seed,
                    lot_id: schedule.lot_id,
                    epoch_id,
                    events,
                    acknowledged_at: Instant::now(),
                })
                .map_err(|_| anyhow!("benchmark consumer stopped before Fabric producer"))?;
        }
        Ok(())
    })
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
    Ok(())
}

fn create_output(path: &PathBuf) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    File::create(path).with_context(|| format!("create {}", path.display()))
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
}
