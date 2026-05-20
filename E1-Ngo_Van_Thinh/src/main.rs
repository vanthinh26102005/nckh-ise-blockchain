mod metrics;
mod recursive_wrapper;
mod threshold_lite;

use anyhow::{bail, Result};
use metrics::{write_csv, SmokeMetric};
use recursive_wrapper::prove_recursive_wrapper;
use threshold_lite::{prove_threshold_trace, ThresholdTrace};

fn main() -> Result<()> {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("week1-smoke") => run_week1_smoke(),
        Some(other) => bail!("unknown command: {other}. Use `week1-smoke`."),
        None => {
            eprintln!("Usage: cargo +nightly run --release -- week1-smoke");
            Ok(())
        }
    }
}

fn run_week1_smoke() -> Result<()> {
    let event_counts = [8_usize, 16, 32, 64];
    let seeds = [0_u64, 1, 2];
    let mut rows = Vec::with_capacity(event_counts.len() * seeds.len());

    for events_per_lot in event_counts {
        for seed in seeds {
            println!("running threshold-lite events_per_lot={events_per_lot} seed={seed}");
            let trace = ThresholdTrace::synthetic(events_per_lot, seed);
            let inner = prove_threshold_trace(&trace)?;
            let recursive = prove_recursive_wrapper(&inner)?;
            let _recursive_shape = (
                recursive.public_input_count(),
                recursive.circuit_degree_bits(),
            );

            rows.push(SmokeMetric {
                circuit: "threshold-lite".to_string(),
                events_per_lot,
                seed,
                prove_ms: inner.prove_ms,
                verify_ms: inner.verify_ms,
                proof_bytes: inner.proof_bytes,
                recursive_prove_ms: recursive.prove_ms,
                recursive_verify_ms: recursive.verify_ms,
                recursive_proof_bytes: recursive.proof_bytes,
            });
        }
    }

    write_csv("results/week1_smoke.csv", &rows)?;
    println!("wrote {} rows to results/week1_smoke.csv", rows.len());
    Ok(())
}
