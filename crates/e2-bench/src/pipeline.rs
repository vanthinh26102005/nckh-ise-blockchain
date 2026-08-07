use crate::l1_adapter::L1Adapter;
use crate::types::{E2Options, LatencyRow, ProfileMode};
use crate::workload::{accumulate_lots, generate_poisson_event_stream};
use anyhow::Result;
use e1_bench::synthetic::synthetic_lot;
use e1_bench::templates::{build_template, prove_and_verify};
use e1_bench::types::{CircuitKind, Profile as E1Profile};
use std::collections::HashMap;

pub fn run_pipeline_seed(
    seed: usize,
    options: &E2Options,
    l1_adapter: &dyn L1Adapter,
) -> Result<Vec<LatencyRow>> {
    let events = generate_poisson_event_stream(
        seed as u64,
        options.lambda_events_per_min,
        options.duration_min,
    );
    let lots = accumulate_lots(&events, seed as u64);

    let e1_profile = match options.profile {
        ProfileMode::Quick => E1Profile::CoffeeSmall,
        ProfileMode::Full => E1Profile::CoffeeDefault,
    };

    let mut rows = Vec::new();
    let mut template_cache = HashMap::new();
    let mut proof_timing_cache = HashMap::new();

    let inner_circuits = [
        CircuitKind::C2,
        CircuitKind::C3,
        CircuitKind::C4,
        CircuitKind::C5,
    ];

    let l1_mode_str = options.l1_mode.to_string();

    // Timeline tracker: serializes lot proving. Timings are measured once per circuit/lot size.
    let mut current_timeline_ms = 0u64;

    for lot in &lots {
        let epoch_id = lot.lot_id; // 1 epoch = 1 lot in MVP base proof pipeline
        let lot_size = lot.events.len();

        // 1. Proof start time: after lot is ready and previous prover job completes
        let proof_start_ms = lot.lot_ready_at_ms.max(current_timeline_ms);

        // Build or reuse E1 templates for this lot size
        for &kind in &inner_circuits {
            if !template_cache.contains_key(&(kind, lot_size)) {
                let template = build_template(kind, lot_size, e1_profile)?;
                template_cache.insert((kind, lot_size), template);
            }
        }

        // Generate synthetic E1 lot & execute proofs C2..C5
        let synth_lot = synthetic_lot(seed as u64 + lot.lot_id as u64, lot_size, e1_profile);
        let mut total_proof_ms = 0.0f64;
        let mut status = "ok".to_string();
        let mut note_msg =
            "base_proof_pipeline_c2_to_c5;proof_timing=witness_ms_plus_prove_ms_cached_by_lot_size"
                .to_string();

        for &kind in &inner_circuits {
            let cache_key = (kind, lot_size);
            if let Some(ms) = proof_timing_cache.get(&cache_key) {
                total_proof_ms += ms;
                continue;
            }

            let tmpl = &template_cache[&cache_key];
            match prove_and_verify(tmpl, &synth_lot, seed, e1_profile) {
                Ok(stats) => {
                    let proof_ms = stats.witness_ms + stats.prove_ms;
                    proof_timing_cache.insert(cache_key, proof_ms);
                    total_proof_ms += proof_ms;
                }
                Err(err) => {
                    status = "error".to_string();
                    note_msg = format!("e1_proof_error_{kind:?}:{err:#}");
                    break;
                }
            }
        }

        let proof_duration_ms = total_proof_ms.ceil() as u64;
        let proof_end_ms = proof_start_ms + proof_duration_ms.max(1);

        // 2. Submit transaction to L1
        let submit_tx_at_ms = proof_end_ms;
        let (confirmed_at_ms, l1_note) =
            l1_adapter.submit_and_confirm(epoch_id, lot.lot_id, submit_tx_at_ms)?;

        current_timeline_ms = proof_end_ms;

        let note = format!("{note_msg};l1={l1_note}");

        // Create a row for each event in this lot
        for event in &lot.events {
            let total_latency_ms = confirmed_at_ms.saturating_sub(event.ingest_at_ms);

            rows.push(LatencyRow {
                seed,
                event_id: event.event_id,
                lot_id: lot.lot_id,
                epoch_id,
                l1_mode: l1_mode_str.clone(),
                ingest_at_ms: event.ingest_at_ms,
                lot_ready_at_ms: lot.lot_ready_at_ms,
                proof_start_ms,
                proof_end_ms,
                submit_tx_at_ms,
                confirmed_at_ms,
                total_latency_ms,
                status: status.clone(),
                note: note.clone(),
            });
        }
    }

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l1_adapter::create_l1_adapter;
    use crate::types::L1Mode;

    #[test]
    fn test_pipeline_seed_run() {
        let options = E2Options {
            profile: ProfileMode::Quick,
            lambda_events_per_min: 480.0,
            duration_min: 1.0,
            seeds: 1,
            l1_mode: L1Mode::Mock,
            out_raw: "test_raw.csv".into(),
            out_summary: "test_sum.csv".into(),
            out_metadata: "test_meta.json".into(),
        };

        let l1_adapter = create_l1_adapter(L1Mode::Mock);
        let rows = run_pipeline_seed(1, &options, l1_adapter.as_ref()).unwrap();

        assert!(!rows.is_empty());
        for row in &rows {
            assert_eq!(row.seed, 1);
            assert_eq!(row.status, "ok");
            assert!(row.lot_ready_at_ms >= row.ingest_at_ms);
            assert!(row.proof_start_ms >= row.lot_ready_at_ms);
            assert!(row.proof_end_ms >= row.proof_start_ms);
            assert!(row.submit_tx_at_ms >= row.proof_end_ms);
            assert!(row.confirmed_at_ms >= row.submit_tx_at_ms);
            assert_eq!(row.total_latency_ms, row.confirmed_at_ms - row.ingest_at_ms);
        }
    }
}
