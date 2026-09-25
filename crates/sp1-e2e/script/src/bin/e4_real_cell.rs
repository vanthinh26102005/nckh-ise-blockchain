//! One paced E4 cell: Fabric -> compressed leaves -> recursive SP1 epoch -> Anvil.
use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use e2_bench::generate_poisson_event_stream;
use eudr_policy::{evaluate, nullifier_index, SparseNullifierMap};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sp1_e2e::{
    benchmark_events_for_lot, benchmark_nullifier_secret,
    http::{FabricGateway, HttpEndpoint},
    policy_input_from_events, prove_epoch_tree_with, prove_policy_compressed_with, prove_with,
    setup_epoch_prover, setup_policy_prover, EventWitness,
};
use sp1_sdk::{
    blocking::{Prover, ProverClient},
    HashableKey, ProvingKey, SP1ProofWithPublicValues,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    np: u64,
    #[arg(long)]
    shipment: usize,
    #[arg(long)]
    epoch_s: u64,
    #[arg(long, default_value_t = 4)]
    epochs: u64,
    #[arg(long)]
    seed: u64,
    #[arg(long)]
    wall_time_s: Option<u64>,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fabric_gateway: String,
    #[arg(long, default_value = "http://127.0.0.1:8546")]
    anchor_server: String,
    #[arg(long, value_enum, default_value_t = ProverKind::Cpu)]
    prover: ProverKind,
    #[arg(long, value_enum, default_value_t = Mode::Proposed)]
    mode: Mode,
}

#[derive(Clone, Copy, ValueEnum)]
enum ProverKind {
    Cpu,
    Cuda,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Mode {
    Proposed,
    FabricOnly,
    HashOnChain,
    VecroAdapted,
}

struct LeafTask {
    epoch_id: u64,
    lot_id: u64,
    shipment_id: u64,
    shipment_end: bool,
    events: Vec<EventWitness>,
    due_ms: Vec<u64>,
    acknowledged_at: Option<Instant>,
}

enum Message {
    Leaf(LeafTask),
    EpochEnd(u64),
}

const FABRIC_WORKERS: usize = 32;

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.np == 0
        || ![16, 32, 64, 128].contains(&cli.shipment)
        || cli.epoch_s == 0
        || cli.epochs == 0
    {
        bail!("invalid E4 cell axes");
    }
    if matches!(cli.mode, Mode::FabricOnly | Mode::HashOnChain) {
        return run_baseline(cli);
    }
    match cli.prover {
        ProverKind::Cpu => run(cli, ProverClient::builder().cpu().build()),
        ProverKind::Cuda => run_cuda(cli),
    }
}

#[cfg(feature = "cuda")]
fn run_cuda(cli: Cli) -> Result<()> {
    run(cli, ProverClient::builder().cuda().build())
}
#[cfg(not(feature = "cuda"))]
fn run_cuda(_: Cli) -> Result<()> {
    bail!("CUDA prover requires --features cuda")
}

fn run<P: Prover>(cli: Cli, client: P) -> Result<()> {
    let fabric = FabricGateway::from_url(&cli.fabric_gateway)?;
    let anchor = HttpEndpoint::from_url(&cli.anchor_server)?;
    let leaf_key = setup_policy_prover(&client).map_err(|error| anyhow!(error))?;
    let epoch_key = setup_epoch_prover(&client).map_err(|error| anyhow!(error))?;
    let mut nullifiers = SparseNullifierMap::default();
    let initial_root = bytes32(nullifiers.root());
    let vkey = epoch_key.verifying_key().bytes32();
    let reset = json!({
        "initialNullifierRoot": initial_root, "programVKey": vkey,
        "leafVKeyDigest": bytes32(leaf_key.verifying_key().hash_bytes()),
        "aggregateVKeyDigest": bytes32(epoch_key.verifying_key().hash_bytes()),
    });
    let reset_path = if cli.mode == Mode::VecroAdapted {
        "/e3/vecro/reset"
    } else {
        "/e3/reset"
    };
    let reset = if cli.mode == Mode::VecroAdapted {
        json!({"initialNullifierRoot": initial_root, "programVKey": leaf_key.verifying_key().bytes32()})
    } else {
        reset
    };
    let response = anchor.request("POST", reset_path, Some(&reset.to_string()))?;
    if response.status != 201 {
        bail!("Anvil E3 reset failed: {}", response.body);
    }
    let tasks = schedule(&cli)?;
    let started = Instant::now();
    let deadline = cli
        .wall_time_s
        .map(|seconds| started + Duration::from_secs(seconds));
    let offered_events = tasks
        .iter()
        .flat_map(|epoch| epoch.iter())
        .map(|leaf| leaf.events.len())
        .sum::<usize>();
    let offered_shipments = tasks
        .iter()
        .flat_map(|epoch| epoch.iter())
        .filter(|leaf| leaf.shipment_end)
        .count();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let producer_stop = stop.clone();
    let producer =
        thread::spawn(move || produce(tasks, fabric, sender, started, deadline, producer_stop));

    let mut leaves: Vec<SP1ProofWithPublicValues> = Vec::new();
    let mut acknowledged: Vec<(Instant, usize)> = Vec::new();
    let mut leaf_ms = Vec::new();
    let mut aggregate_ms = Vec::new();
    let mut audit_ms = Vec::new();
    let mut gas = Vec::new();
    let mut calldata = Vec::new();
    let mut transactions = Vec::new();
    let mut committed_events = 0_usize;
    let mut committed_shipments = 0_usize;
    let mut pending_shipments = 0_usize;
    let mut epochs_completed = 0_u64;
    let mut reason = None;
    let mut saturated = false;

    for message in receiver {
        if deadline.is_some_and(|end| Instant::now() >= end) {
            saturated = true;
            stop.store(true, Ordering::Relaxed);
            break;
        }
        let step: Result<()> = match message {
            Message::Leaf(task) => (|| {
                let secret = benchmark_nullifier_secret(cli.seed, task.lot_id);
                let index = nullifier_index(task.lot_id, secret);
                let input = policy_input_from_events(
                    task.epoch_id,
                    task.lot_id,
                    task.events,
                    nullifiers.root(),
                    secret,
                    nullifiers
                        .empty_path(index)
                        .map_err(|error| anyhow!("C5 path: {error:?}"))?,
                )
                .map_err(|error| anyhow!(error))?;
                let expected = evaluate(&input).map_err(|error| anyhow!("policy: {error:?}"))?;
                let proof_started = Instant::now();
                let proof = if cli.mode == Mode::VecroAdapted {
                    prove_with(&client, &leaf_key, &input)
                } else {
                    prove_policy_compressed_with(&client, &leaf_key, &input)
                }
                .map_err(|error| anyhow!(error))?;
                leaf_ms.push(proof_started.elapsed().as_millis() as f64);
                nullifiers
                    .insert(index)
                    .map_err(|error| anyhow!("C5 insert: {error:?}"))?;
                if nullifiers.root() != expected.new_nullifier_root {
                    bail!("C5 map diverged");
                }
                let acknowledged_at = task
                    .acknowledged_at
                    .context("missing Fabric acknowledgement")?;
                if cli.mode == Mode::VecroAdapted {
                    let body = json!({
                        "programVKey": leaf_key.verifying_key().bytes32(),
                        "publicValues": bytes32_slice(proof.public_values.as_slice()),
                        "proof": bytes32_slice(&proof.bytes()),
                    });
                    let response =
                        anchor.request("POST", "/e3/vecro/mint", Some(&body.to_string()))?;
                    if response.status != 201 {
                        bail!("adapted token mint rejected: {}", response.body);
                    }
                    let receipt: Value = serde_json::from_str(&response.body)?;
                    if receipt["newNullifierRoot"].as_str()
                        != Some(bytes32(nullifiers.root()).as_str())
                    {
                        bail!("adapted token root disagrees with SP1 policy proof");
                    }
                    audit_ms.push(acknowledged_at.elapsed().as_millis() as f64);
                    committed_events += input.events.len();
                    if task.shipment_end {
                        committed_shipments += 1;
                    }
                    gas.push(
                        receipt["gasUsed"]
                            .as_str()
                            .context("missing gasUsed")?
                            .parse::<f64>()?,
                    );
                    calldata.push(
                        receipt["calldataBytes"]
                            .as_u64()
                            .context("missing calldataBytes")? as f64,
                    );
                    transactions.push(receipt);
                } else {
                    if task.shipment_end {
                        pending_shipments += 1;
                    }
                    acknowledged.push((acknowledged_at, input.events.len()));
                    leaves.push(proof);
                }
                Ok(())
            })(),
            Message::EpochEnd(epoch_id) => (|| {
                if cli.mode == Mode::VecroAdapted {
                    epochs_completed += 1;
                    return Ok(());
                }
                if leaves.is_empty() {
                    bail!("epoch has no provable leaves");
                }
                let old_root = leaves[0].public_values.as_slice()[32..64].to_vec();
                let proof_started = Instant::now();
                let proof = prove_epoch_tree_with(
                    &client,
                    &epoch_key,
                    &leaf_key,
                    std::mem::take(&mut leaves),
                )
                .map_err(|error| anyhow!(error))?;
                aggregate_ms.push(proof_started.elapsed().as_millis() as f64);
                let body = json!({
                    "epochId": epoch_id.to_string(), "oldNullifierRoot": bytes32_slice(&old_root),
                    "programVKey": vkey,
                    "publicValues": bytes32_slice(proof.public_values.as_slice()),
                    "proof": bytes32_slice(&proof.bytes()),
                });
                let response = anchor.request("POST", "/e3/anchor", Some(&body.to_string()))?;
                if response.status != 201 {
                    bail!("Anvil E3 anchor rejected: {}", response.body);
                }
                let receipt: Value = serde_json::from_str(&response.body)?;
                if receipt["newNullifierRoot"].as_str() != Some(bytes32(nullifiers.root()).as_str())
                {
                    bail!("Anvil epoch root disagrees with SP1 proof");
                }
                for (time, count) in acknowledged.drain(..) {
                    audit_ms.push(time.elapsed().as_millis() as f64);
                    committed_events += count;
                }
                committed_shipments += pending_shipments;
                pending_shipments = 0;
                gas.push(
                    receipt["gasUsed"]
                        .as_str()
                        .context("missing gasUsed")?
                        .parse::<f64>()?,
                );
                calldata.push(
                    receipt["calldataBytes"]
                        .as_u64()
                        .context("missing calldataBytes")? as f64,
                );
                transactions.push(receipt);
                epochs_completed += 1;
                Ok(())
            })(),
        };
        if let Err(error) = step {
            reason = Some(error.to_string());
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    let producer_result = producer
        .join()
        .map_err(|_| anyhow!("Fabric producer panicked"))?;
    if let Err(error) = producer_result {
        reason.get_or_insert(error.to_string());
    }
    let elapsed_s = started.elapsed().as_secs_f64();
    saturated |= deadline.is_some_and(|end| Instant::now() >= end);
    let complete = reason.is_none()
        && !saturated
        && epochs_completed == cli.epochs
        && committed_events == offered_events
        && committed_shipments == offered_shipments;
    println!(
        "{}",
        json!({
            "mode": if cli.mode == Mode::VecroAdapted { "vecro-adapted" } else { "proposed" },
        "status": if complete { "ok" } else if reason.is_some() { "censored" } else if saturated { "saturated" } else { "censored" },
            "censor_reason": reason.or_else(|| if complete { None } else { Some("wall_time_or_incomplete_epoch".to_string()) }),
            "epochs_completed": epochs_completed,
            "queue_drained": complete,
            "elapsed_s": elapsed_s,
            "offered_events": offered_events,
            "offered_shipments": offered_shipments,
            "committed_events": committed_events,
            "committed_shipments": committed_shipments,
            "leaf_samples": leaf_ms,
            "aggregate_samples": aggregate_ms,
            "audit_samples": audit_ms,
            "gas_samples": gas,
            "calldata_samples": calldata,
            "transactions": transactions,
            "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
        "rng": "ChaCha20Rng seed from e2-bench workload",
        "fabricWorkers": FABRIC_WORKERS,
        "policyProgramVKey": leaf_key.verifying_key().bytes32(),
            "aggregateProgramVKey": vkey,
        })
    );
    Ok(())
}

fn run_baseline(cli: Cli) -> Result<()> {
    let fabric = FabricGateway::from_url(&cli.fabric_gateway)?;
    let anchor = if cli.mode == Mode::HashOnChain {
        let endpoint = HttpEndpoint::from_url(&cli.anchor_server)?;
        let response = endpoint.request("POST", "/e3/hash/reset", Some("{}"))?;
        if response.status != 201 {
            bail!("hash baseline reset failed: {}", response.body);
        }
        Some(endpoint)
    } else {
        None
    };
    let tasks = schedule(&cli)?;
    let offered_events = tasks
        .iter()
        .flat_map(|epoch| epoch.iter())
        .map(|leaf| leaf.events.len())
        .sum::<usize>();
    let offered_shipments = tasks
        .iter()
        .flat_map(|epoch| epoch.iter())
        .filter(|leaf| leaf.shipment_end)
        .count();
    let started = Instant::now();
    let deadline = cli
        .wall_time_s
        .map(|seconds| started + Duration::from_secs(seconds));
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let producer_stop = stop.clone();
    let producer =
        thread::spawn(move || produce(tasks, fabric, sender, started, deadline, producer_stop));
    let mut digest = Sha256::new();
    let mut pending_events = 0_usize;
    let mut pending_since = None;
    let mut committed_events = 0_usize;
    let mut committed_shipments = 0_usize;
    let mut epochs_completed = 0_u64;
    let mut audit_ms = Vec::new();
    let mut gas = Vec::new();
    let mut calldata = Vec::new();
    let mut transactions = Vec::new();
    let mut reason = None;
    let mut saturated = false;
    for message in receiver {
        if deadline.is_some_and(|end| Instant::now() >= end) {
            saturated = true;
            stop.store(true, Ordering::Relaxed);
            break;
        }
        let step: Result<()> = match message {
            Message::Leaf(task) => (|| {
                if pending_events == 0 {
                    digest.update(b"EUDR:E3:SHIPMENT:V1");
                }
                for witness in &task.events {
                    digest.update(witness.event_bytes);
                }
                pending_events += task.events.len();
                pending_since.get_or_insert(
                    task.acknowledged_at
                        .context("missing Fabric acknowledgement")?,
                );
                if task.shipment_end {
                    if let Some(endpoint) = &anchor {
                        let body = json!({"shipmentId": task.shipment_id.to_string(),
                            "digest": bytes32_slice(&digest.finalize_reset()), "eventCount": pending_events});
                        let response =
                            endpoint.request("POST", "/e3/hash/anchor", Some(&body.to_string()))?;
                        if response.status != 201 {
                            bail!("hash anchor rejected: {}", response.body);
                        }
                        let receipt: Value = serde_json::from_str(&response.body)?;
                        gas.push(
                            receipt["gasUsed"]
                                .as_str()
                                .context("missing gasUsed")?
                                .parse::<f64>()?,
                        );
                        calldata.push(
                            receipt["calldataBytes"]
                                .as_u64()
                                .context("missing calldataBytes")?
                                as f64,
                        );
                        transactions.push(receipt);
                    } else {
                        digest.reset();
                    }
                    audit_ms.push(pending_since.take().unwrap().elapsed().as_millis() as f64);
                    committed_events += pending_events;
                    committed_shipments += 1;
                    pending_events = 0;
                }
                Ok(())
            })(),
            Message::EpochEnd(_) => {
                if pending_events != 0 {
                    Err(anyhow!("epoch ended mid-shipment"))
                } else {
                    epochs_completed += 1;
                    Ok(())
                }
            }
        };
        if let Err(error) = step {
            reason = Some(error.to_string());
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    let producer_result = producer
        .join()
        .map_err(|_| anyhow!("Fabric producer panicked"))?;
    if let Err(error) = producer_result {
        reason.get_or_insert(error.to_string());
    }
    let elapsed_s = started.elapsed().as_secs_f64();
    saturated |= deadline.is_some_and(|end| Instant::now() >= end);
    let complete = reason.is_none()
        && !saturated
        && epochs_completed == cli.epochs
        && committed_events == offered_events
        && committed_shipments == offered_shipments;
    println!(
        "{}",
        json!({
            "mode": match cli.mode { Mode::FabricOnly => "fabric-only", Mode::HashOnChain => "hash-on-chain", _ => unreachable!() },
        "status": if complete { "ok" } else if reason.is_some() { "censored" } else if saturated { "saturated" } else { "censored" },
            "censor_reason": reason.or_else(|| if complete { None } else { Some("wall_time_or_incomplete_epoch".to_string()) }),
            "epochs_completed": epochs_completed, "queue_drained": complete,
            "elapsed_s": elapsed_s, "offered_events": offered_events, "committed_events": committed_events,
            "offered_shipments": offered_shipments,
            "committed_shipments": committed_shipments,
            "leaf_samples": [], "aggregate_samples": [], "audit_samples": audit_ms,
            "gas_samples": gas, "calldata_samples": calldata, "transactions": transactions,
            "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
            "rng": "ChaCha20Rng seed from e2-bench workload",
            "fabricWorkers": FABRIC_WORKERS,
        })
    );
    Ok(())
}

fn produce(
    tasks: Vec<Vec<LeafTask>>,
    gateway: FabricGateway,
    sender: mpsc::Sender<Message>,
    started: Instant,
    deadline: Option<Instant>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let total = tasks.iter().map(Vec::len).sum::<usize>();
    let workers = FABRIC_WORKERS.min(total.max(1));
    let mut assignments: Vec<Vec<(usize, LeafTask)>> = (0..workers).map(|_| Vec::new()).collect();
    let mut epoch_ends = BTreeMap::new();
    let mut index = 0;
    for epoch in tasks {
        let epoch_id = epoch.first().context("empty epoch")?.epoch_id;
        for leaf in epoch {
            assignments[index % workers].push((index, leaf));
            index += 1;
        }
        epoch_ends.insert(index - 1, epoch_id);
    }

    let (result_sender, result_receiver) = mpsc::channel::<(usize, Result<LeafTask>)>();
    let handles = assignments
        .into_iter()
        .map(|assigned| {
            let gateway = gateway.clone();
            let result_sender = result_sender.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                for (index, mut leaf) in assigned {
                    let result: Result<()> = (|| {
                        for (event, due_ms) in leaf.events.iter().zip(&leaf.due_ms) {
                            let due = started + Duration::from_millis(*due_ms);
                            loop {
                                if stop.load(Ordering::Relaxed)
                                    || deadline.is_some_and(|end| Instant::now() >= end)
                                {
                                    return Ok(());
                                }
                                match due.checked_duration_since(Instant::now()) {
                                    Some(wait) => {
                                        thread::sleep(wait.min(Duration::from_millis(100)))
                                    }
                                    None => break,
                                }
                            }
                            gateway.post_event(&event.event_bytes)?;
                            gateway.assert_event_was_committed(&event.event_bytes)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        stop.store(true, Ordering::Relaxed);
                        let _ = result_sender.send((index, Err(error)));
                        return;
                    }
                    if stop.load(Ordering::Relaxed)
                        || deadline.is_some_and(|end| Instant::now() >= end)
                    {
                        return;
                    }
                    leaf.acknowledged_at = Some(Instant::now());
                    if result_sender.send((index, Ok(leaf))).is_err() {
                        return;
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    drop(result_sender);

    let mut ready = BTreeMap::new();
    let mut next = 0;
    let mut failure = None;
    for (index, result) in result_receiver {
        match result {
            Ok(leaf) => {
                ready.insert(index, leaf);
            }
            Err(error) => {
                failure = Some(error);
                stop.store(true, Ordering::Relaxed);
                break;
            }
        }
        while let Some(leaf) = ready.remove(&next) {
            if sender.send(Message::Leaf(leaf)).is_err() {
                stop.store(true, Ordering::Relaxed);
                break;
            }
            if let Some(epoch_id) = epoch_ends.get(&next) {
                if sender.send(Message::EpochEnd(*epoch_id)).is_err() {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
            next += 1;
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    for handle in handles {
        handle
            .join()
            .map_err(|_| anyhow!("Fabric worker panicked"))?;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if next < total && !deadline.is_some_and(|end| Instant::now() >= end) {
        bail!("Fabric producer stopped before all scheduled leaves were acknowledged");
    }
    Ok(())
}

fn schedule(cli: &Cli) -> Result<Vec<Vec<LeafTask>>> {
    let duration_min = cli.epoch_s as f64 * cli.epochs as f64 / 60.0;
    let arrivals = generate_poisson_event_stream(cli.seed, cli.np as f64 * 3.84, duration_min);
    let mut epochs = Vec::new();
    let mut event_number = 0_u64;
    let mut leaf_number = 0_u64;
    let mut shipment_number = 0_u64;
    let mode_number = match cli.mode {
        Mode::Proposed => 0,
        Mode::FabricOnly => 1,
        Mode::HashOnChain => 2,
        Mode::VecroAdapted => 3,
    };
    for epoch_index in 0..cli.epochs {
        let start_ms = epoch_index * cli.epoch_s * 1000;
        let end_ms = start_ms + cli.epoch_s * 1000;
        let due: Vec<u64> = arrivals
            .iter()
            .filter(|event| start_ms <= event.ingest_at_ms && event.ingest_at_ms < end_ms)
            .map(|event| event.ingest_at_ms)
            .collect();
        if due.len() < 8 {
            bail!("epoch {epoch_index} has fewer than eight offered events");
        }
        let epoch_id = cli.seed * 1000 + mode_number * 200 + epoch_index;
        let mut batches = Vec::new();
        let mut cursor = 0;
        for size in shipment_sizes(due.len(), cli.shipment) {
            let end = cursor + size;
            let shipment_id = cli.seed * 100_000 + mode_number * 20_000 + shipment_number;
            let mut part = cursor;
            while part < end {
                let remaining = end - part;
                let leaf_end = part
                    + if remaining <= 64 {
                        remaining
                    } else {
                        64.min(remaining - 8)
                    };
                let count = leaf_end - part;
                if count < 8 {
                    bail!("shipment split produced a leaf smaller than eight events");
                }
                let lot_id = cli.seed * 100_000 + mode_number * 20_000 + leaf_number;
                let first_event_id = cli.seed * 200_000 + mode_number * 40_000 + event_number;
                let events =
                    benchmark_events_for_lot(cli.seed, epoch_id, lot_id, first_event_id, count)
                        .map_err(|error| anyhow!(error))?;
                batches.push(LeafTask {
                    epoch_id,
                    lot_id,
                    shipment_id,
                    shipment_end: leaf_end == end,
                    events,
                    due_ms: due[part..leaf_end].to_vec(),
                    acknowledged_at: None,
                });
                event_number += count as u64;
                leaf_number += 1;
                part = leaf_end;
            }
            cursor = end;
            shipment_number += 1;
        }
        epochs.push(batches);
    }
    Ok(epochs)
}

fn shipment_sizes(events: usize, shipment: usize) -> Vec<usize> {
    let mut sizes = vec![shipment; events / shipment];
    let tail = events % shipment;
    if tail >= 8 {
        sizes.push(tail);
    } else if tail > 0 {
        if let Some(last) = sizes.last_mut() {
            *last -= 8 - tail;
            sizes.push(8);
        }
    }
    sizes
}

fn bytes32(value: [u8; 32]) -> String {
    bytes32_slice(&value)
}
fn bytes32_slice(value: &[u8]) -> String {
    format!("0x{}", hex::encode(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cli(mode: Mode) -> Cli {
        Cli {
            np: 125,
            shipment: 128,
            epoch_s: 60,
            epochs: 1,
            seed: 17,
            wall_time_s: None,
            fabric_gateway: "http://127.0.0.1:8080".to_string(),
            anchor_server: "http://127.0.0.1:8546".to_string(),
            prover: ProverKind::Cpu,
            mode,
        }
    }

    #[test]
    fn paired_modes_share_arrivals_and_split_128_shipments() {
        let proposed = schedule(&test_cli(Mode::Proposed)).unwrap();
        let baseline = schedule(&test_cli(Mode::VecroAdapted)).unwrap();
        let first = &proposed[0];
        let second = &baseline[0];
        let arrivals = |leaves: &[LeafTask]| {
            leaves
                .iter()
                .flat_map(|leaf| leaf.due_ms.iter().copied())
                .collect::<Vec<_>>()
        };
        assert_eq!(arrivals(first), arrivals(second));
        assert_ne!(first[0].lot_id, second[0].lot_id);
        assert!(first
            .iter()
            .all(|leaf| (8..=64).contains(&leaf.events.len())));
        assert!(first.iter().any(|leaf| !leaf.shipment_end));
        assert_eq!(
            first.iter().filter(|leaf| leaf.shipment_end).count(),
            shipment_sizes(arrivals(first).len(), 128).len()
        );
    }

    #[test]
    fn shipment_tail_and_128_split_keep_leaf_bounds() {
        for size in [16, 32, 64, 128] {
            for total in size..size * 3 {
                let groups = shipment_sizes(total, size);
                assert_eq!(groups.iter().sum::<usize>(), total);
                assert!(groups.iter().all(|n| (8..=size).contains(n)));
            }
        }
    }
}
