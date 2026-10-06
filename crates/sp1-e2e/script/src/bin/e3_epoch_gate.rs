//! Two real Fabric lots -> compressed policy proofs -> recursive epoch proof.
//! The generated fixture is submitted to Anvil by contracts/test/anvil_e3_smoke.mjs.
use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use eudr_policy::{epoch::EPOCH_PUBLIC_BYTES, evaluate, nullifier_index, SparseNullifierMap};
use serde_json::json;
use sha2::{Digest,Sha256};
use sp1_e2e::{
    benchmark_registry, benchmark_owner, benchmark_batch_nonce, benchmark_events_for_lot, benchmark_nullifier_key, http::FabricGateway,
    policy_input_from_events, prove_epoch_with, prove_policy_compressed_with, setup_epoch_prover,
    setup_policy_prover,
};
use sp1_sdk::{
    blocking::{Prover, ProverClient},
    HashableKey, ProvingKey, SP1PublicValues,
};
use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    fabric_gateway: String,
    #[arg(long, default_value = "results/e3/pilot/epoch-proof.json")]
    out: PathBuf,
    #[arg(long)]
    epoch_id: Option<u64>,
    #[arg(long, default_value_t = 8)]
    events_per_leaf: usize,
    #[arg(long, value_enum, default_value_t = ProverKind::Cpu)]
    prover: ProverKind,
    /// Reuse the exact Fabric events committed by the matching CPU preflight.
    #[arg(long, default_value_t = false)]
    reuse_fabric_evidence: bool,
    /// Write trusted public policy configuration before proving/deployment.
    #[arg(long)]
    prepare_only: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum ProverKind {
    Cpu,
    Cuda,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.out.exists() { bail!("output already exists"); }
    if cli.prepare_only {
        return match cli.prover { ProverKind::Cpu=>prepare(&cli,&ProverClient::builder().cpu().build()),ProverKind::Cuda=>prepare_cuda(&cli) };
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
    bail!("CUDA prover requires --features cuda in the Slurm GPU allocation")
}

fn run<P: Prover>(cli: Cli, client: P) -> Result<()> {
    if ![8, 64].contains(&cli.events_per_leaf) {
        bail!("--events-per-leaf must be 8 or 64 for the E3/E4 preflight");
    }
    let gateway = FabricGateway::from_url(&cli.fabric_gateway)?;
    let epoch_id = cli.epoch_id.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_secs()
    });
    let setup_started = Instant::now();
    let leaf_key = setup_policy_prover(&client).map_err(|error| anyhow!(error))?;
    let epoch_key = setup_epoch_prover(&client).map_err(|error| anyhow!(error))?;
    let setup_ms=setup_started.elapsed().as_secs_f64()*1000.0;
    let mut nullifiers = SparseNullifierMap::default();
    let initial_root = nullifiers.root();
    let mut proofs = Vec::new();
    let mut leaf_ms = Vec::new();
    let mut witness_ms=Vec::new();let mut leaf_serialized_bytes=Vec::new();
    let registry_started=Instant::now();

    let planned = planned_events(epoch_id,cli.events_per_leaf)?;
    let all_events: Vec<_> = planned.iter().flat_map(|(_,events)|events.clone()).collect();
    let registry = benchmark_registry(epoch_id,&all_events).map_err(|e|anyhow!(e))?;
    sp1_e2e::require_registry_configuration(&registry,&leaf_key.verifying_key().bytes32(),&epoch_key.verifying_key().bytes32(),leaf_key.verifying_key().hash_bytes(),epoch_key.verifying_key().hash_bytes()).map_err(|e|anyhow!(e))?;
    let registry_ms=registry_started.elapsed().as_secs_f64()*1000.0;
    for (lot_id,events) in planned {
        for witness in &events {
            if !cli.reuse_fabric_evidence {
                gateway.post_event(&witness.event_bytes)?;
            }
            gateway.assert_event_was_committed(&witness.event_bytes)?;
        }
        let witness_started=Instant::now();
        let secret = benchmark_nullifier_key(epoch_id, lot_id);
        let index = nullifier_index(lot_id, secret);
        let input = policy_input_from_events(
            epoch_id,
            lot_id,
            events,
            nullifiers.root(),
            secret,
            nullifiers
                .empty_path(index)
                .map_err(|error| anyhow!("C5 path: {error:?}"))?,
            &registry,benchmark_owner(epoch_id),benchmark_batch_nonce(epoch_id,lot_id),
        )
        .map_err(|error| anyhow!(error))?;
        let expected = evaluate(&input).map_err(|error| anyhow!("policy: {error:?}"))?;
        witness_ms.push(witness_started.elapsed().as_secs_f64()*1000.0);
        let started = Instant::now();
        let proof = prove_policy_compressed_with(&client, &leaf_key, &input)
            .map_err(|error| anyhow!(error))?;
        leaf_ms.push(started.elapsed().as_millis());
        leaf_serialized_bytes.push(serde_json::to_vec(&proof)?.len());
        nullifiers
            .insert(index)
            .map_err(|error| anyhow!("C5 insert: {error:?}"))?;
        if nullifiers.root() != expected.new_nullifier_root {
            bail!("host nullifier map diverged from the proved transition");
        }
        proofs.push(proof);
    }

    let mut tampered = proofs[0].clone();
    let mut wrong_values = tampered.public_values.to_vec();
    wrong_values[96] ^= 1;
    tampered.public_values = SP1PublicValues::from(&wrong_values);
    if client
        .verify(&tampered, leaf_key.verifying_key(), None)
        .is_ok()
    {
        bail!("SP1 accepted a child proof with tampered public values");
    }

    let started = Instant::now();
    let epoch_proof =
        prove_epoch_with(&client, &epoch_key, &leaf_key, proofs).map_err(|error| anyhow!(error))?;
    let aggregate_ms = started.elapsed().as_millis();
    let values: &[u8; EPOCH_PUBLIC_BYTES] = epoch_proof
        .public_values
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("epoch public values are not 352 bytes"))?;
    if values[64..96] != nullifiers.root() {
        bail!("aggregate proof output disagrees with host nullifier map");
    }
    let fixture = json!({
        "protocolVersion": 2, "sourceRevision":sp1_e2e::source_revision(), "verificationKind": "sp1-groth16",
        "certificateRoot": format!("0x{}",hex::encode(registry.root())),
        "polygonCommitment": format!("0x{}",hex::encode(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce))),
        "workloadDigest": format!("0x{}",hex::encode(Sha256::digest(all_events.iter().flat_map(|w|w.event_bytes).collect::<Vec<_>>()))),
        "syntheticFixture": true, "credentialDepth": 16, "nullifierDepth": 256,
        "vkey": epoch_key.verifying_key().bytes32(),
        "leafVKeyDigest": format!("0x{}", hex::encode(leaf_key.verifying_key().hash_bytes())),
        "aggregateVKeyDigest": format!("0x{}", hex::encode(epoch_key.verifying_key().hash_bytes())),
        "publicValues": format!("0x{}", hex::encode(values)),
        "proof": format!("0x{}", hex::encode(epoch_proof.bytes())),
        "epochId": epoch_id.to_string(),
        "oldNullifierRoot": format!("0x{}", hex::encode(initial_root)),
        "newNullifierRoot": format!("0x{}", hex::encode(nullifiers.root())),
        "fabricEventsAcknowledged": 2 * cli.events_per_leaf,
        "tamperedChildRejected": true,
        "leafCompressedProofMs": leaf_ms,
        "setupMs":setup_ms,"registryAndWorkloadMs":registry_ms,"witnessMs":witness_ms,
        "leafSerializedProofBytes":leaf_serialized_bytes,"aggregateProofBytes":epoch_proof.bytes().len(),
        "aggregateCompressedProofMs":null,"groth16WrappingMs":null,"peakRssBytes":null,
        "phaseBoundary":"aggregateGroth16ProofMs combines recursive proving and SDK Groth16 wrapping; unavailable phase/RSS metrics are null",
        "aggregateGroth16ProofMs": aggregate_ms,
        "prover": match cli.prover { ProverKind::Cpu => "cpu", ProverKind::Cuda => "cuda" },
    });
    if let Some(parent) = cli.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cli.out, serde_json::to_vec_pretty(&fixture)?)
        .with_context(|| format!("write {}", cli.out.display()))?;
    println!("E3 proof fixture: {}", cli.out.display());
    Ok(())
}

fn planned_events(epoch_id:u64,event_count:usize) -> Result<Vec<(u64,Vec<eudr_policy::EventWitness>)>> {
    if ![8,64].contains(&event_count) { bail!("event count must be8/64"); }
    let mut planned = Vec::new();
    for offset in 0..2_u64 {
        let lot_id = epoch_id
            .checked_mul(2)
            .and_then(|id| id.checked_add(offset))
            .context("epoch ID too large for unique lot IDs")?;
        let first_event_id = epoch_id
            .checked_mul(128)
            .and_then(|id| id.checked_add(offset * event_count as u64))
            .context("epoch ID too large for unique event IDs")?;
        let events = benchmark_events_for_lot(
            epoch_id,
            epoch_id,
            lot_id,
            first_event_id,
            event_count,
        )
        .map_err(|error| anyhow!(error))?;
        planned.push((lot_id,events));
    }
    Ok(planned)
}

fn prepare<P:Prover>(cli:&Cli,client:&P) -> Result<()> {
    let epoch=cli.epoch_id.context("--prepare-only requires --epoch-id")?;
    let planned=planned_events(epoch,cli.events_per_leaf)?;
    let all:Vec<_>=planned.iter().flat_map(|(_,e)|e.clone()).collect();
    let registry=benchmark_registry(epoch,&all).map_err(|e|anyhow!(e))?;
    let leaf_key=setup_policy_prover(client).map_err(|e|anyhow!(e))?;
    let epoch_key=setup_epoch_prover(client).map_err(|e|anyhow!(e))?;
    let output=json!({"leafProgramVKey":leaf_key.verifying_key().bytes32(),"aggregateProgramVKey":epoch_key.verifying_key().bytes32(),
        "leafVKeyDigest":format!("0x{}",hex::encode(leaf_key.verifying_key().hash_bytes())),
        "aggregateVKeyDigest":format!("0x{}",hex::encode(epoch_key.verifying_key().hash_bytes())),
        "protocolVersion":2,"sourceRevision":sp1_e2e::source_revision(),"validationKind":"synthetic-policy-configuration",
        "syntheticFixture":true,"seed":epoch,"credentialDepth":16,"nullifierDepth":256,
        "certificateRoot":format!("0x{}",hex::encode(registry.root())),
        "polygonCommitment":format!("0x{}",hex::encode(eudr_policy::polygon_commitment(&registry.polygon,registry.polygon_nonce))),
        "workloadDigest":format!("0x{}",hex::encode(Sha256::digest(all.iter().flat_map(|w|w.event_bytes).collect::<Vec<_>>()))) });
    fs::write(&cli.out,serde_json::to_vec_pretty(&output)?)?; Ok(())
}

#[cfg(feature="cuda")]
fn prepare_cuda(cli:&Cli)->Result<()> { prepare(cli,&ProverClient::builder().cuda().build()) }
#[cfg(not(feature="cuda"))]
fn prepare_cuda(_: &Cli)->Result<()> { bail!("CUDA configuration requires --features cuda") }
