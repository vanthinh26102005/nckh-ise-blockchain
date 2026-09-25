//! Two real Fabric lots -> compressed policy proofs -> recursive epoch proof.
//! The generated fixture is submitted to Anvil by contracts/test/anvil_e3_smoke.mjs.
use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, ValueEnum};
use eudr_policy::{epoch::EPOCH_PUBLIC_BYTES, evaluate, nullifier_index, SparseNullifierMap};
use serde_json::json;
use sp1_e2e::{
    benchmark_events_for_lot, benchmark_nullifier_secret, http::FabricGateway,
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
}

#[derive(Clone, Copy, ValueEnum)]
enum ProverKind {
    Cpu,
    Cuda,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
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
    let leaf_key = setup_policy_prover(&client).map_err(|error| anyhow!(error))?;
    let epoch_key = setup_epoch_prover(&client).map_err(|error| anyhow!(error))?;
    let mut nullifiers = SparseNullifierMap::default();
    let initial_root = nullifiers.root();
    let mut proofs = Vec::new();
    let mut leaf_ms = Vec::new();

    for offset in 0..2_u64 {
        let lot_id = epoch_id
            .checked_mul(2)
            .and_then(|id| id.checked_add(offset))
            .context("epoch ID too large for unique lot IDs")?;
        let first_event_id = epoch_id
            .checked_mul(128)
            .and_then(|id| id.checked_add(offset * cli.events_per_leaf as u64))
            .context("epoch ID too large for unique event IDs")?;
        let events = benchmark_events_for_lot(
            epoch_id,
            epoch_id,
            lot_id,
            first_event_id,
            cli.events_per_leaf,
        )
        .map_err(|error| anyhow!(error))?;
        for witness in &events {
            if !cli.reuse_fabric_evidence {
                gateway.post_event(&witness.event_bytes)?;
            }
            gateway.assert_event_was_committed(&witness.event_bytes)?;
        }
        let secret = benchmark_nullifier_secret(epoch_id, lot_id);
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
        )
        .map_err(|error| anyhow!(error))?;
        let expected = evaluate(&input).map_err(|error| anyhow!("policy: {error:?}"))?;
        let started = Instant::now();
        let proof = prove_policy_compressed_with(&client, &leaf_key, &input)
            .map_err(|error| anyhow!(error))?;
        leaf_ms.push(started.elapsed().as_millis());
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
        .map_err(|_| anyhow!("epoch public values are not 288 bytes"))?;
    if values[64..96] != nullifiers.root() {
        bail!("aggregate proof output disagrees with host nullifier map");
    }
    let fixture = json!({
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
