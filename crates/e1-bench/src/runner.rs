use crate::synthetic::synthetic_lot;
#[cfg(not(feature = "recursion"))]
use crate::templates::prove_inner;
#[cfg(feature = "recursion")]
use crate::templates::prove_recursive_wrapper;
use crate::templates::{prove_and_verify, TemplateCache};
use crate::types::{BenchmarkOptions, CircuitKind, MetricRow, Profile};
use anyhow::{bail, Result};
use rayon::prelude::*;
use std::fs;
use std::time::Instant;

pub fn run_benchmark(options: BenchmarkOptions) -> Result<()> {
    if options.jobs == 0 {
        bail!("--jobs must be >= 1");
    }
    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut writer = csv::Writer::from_path(&options.out)?;
    if options.strict_output {
        writer.write_record([
            "circuit",
            "events_per_lot",
            "seed",
            "prove_time_s",
            "verify_time_ms",
            "proof_size_bytes",
            "peak_ram_gb",
        ])?;
    }
    for &events in &options.events_per_lot {
        let setup_start = Instant::now();
        let cache = TemplateCache::build(events, options.profile, &options.circuits)?;
        let setup_ms = setup_start.elapsed().as_secs_f64() * 1000.0;
        let rows = if options.jobs == 1 {
            run_event_grid(
                events,
                options.profile,
                &options.circuits,
                &cache,
                setup_ms,
                options.seeds,
            )
        } else {
            run_event_grid_parallel(
                events,
                options.profile,
                &options.circuits,
                &cache,
                setup_ms,
                options.seeds,
                options.jobs,
            )?
        };
        for row in rows {
            if options.strict_output {
                if row.status != "ok" {
                    bail!(
                        "strict benchmark row failed: seed={} events={} circuit={} note={}",
                        row.seed,
                        row.events_per_lot,
                        row.circuit,
                        row.note
                    );
                }
                writer.write_record([
                    row.circuit,
                    row.events_per_lot.to_string(),
                    row.seed.to_string(),
                    format!("{:.9}", row.prove_ms / 1000.0),
                    format!("{:.6}", row.verify_ms),
                    row.proof_bytes.to_string(),
                    format!("{:.9}", row.peak_rss_mb / 1024.0),
                ])?;
            } else {
                writer.serialize(row)?;
            }
            writer.flush()?;
        }
    }
    Ok(())
}

fn run_event_grid(
    events: usize,
    profile: Profile,
    circuits: &[CircuitKind],
    cache: &TemplateCache,
    setup_ms: f64,
    seeds: usize,
) -> Vec<MetricRow> {
    (1..=seeds)
        .flat_map(|seed| run_seed_rows(seed, events, profile, circuits, cache, setup_ms))
        .collect()
}

fn run_event_grid_parallel(
    events: usize,
    profile: Profile,
    circuits: &[CircuitKind],
    cache: &TemplateCache,
    setup_ms: f64,
    seeds: usize,
    jobs: usize,
) -> Result<Vec<MetricRow>> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let mut rows = pool.install(|| {
        (1..=seeds)
            .into_par_iter()
            .flat_map_iter(|seed| run_seed_rows(seed, events, profile, circuits, cache, setup_ms))
            .collect::<Vec<_>>()
    });
    rows.sort_by_key(|row| {
        (
            row.seed,
            circuits
                .iter()
                .position(|kind| kind.as_str() == row.circuit)
                .unwrap_or(usize::MAX),
        )
    });
    Ok(rows)
}

fn run_seed_rows(
    seed: usize,
    events: usize,
    profile: Profile,
    circuits: &[CircuitKind],
    cache: &TemplateCache,
    setup_ms: f64,
) -> Vec<MetricRow> {
    let lot = synthetic_lot(seed as u64, events, profile);
    circuits
        .iter()
        .map(|&kind| match kind {
            CircuitKind::Wrapper => run_wrapper_row(seed, events, profile, cache, &lot, setup_ms),
            _ => run_regular_row(seed, events, profile, cache, &lot, kind, setup_ms),
        })
        .collect()
}

fn run_regular_row(
    seed: usize,
    events: usize,
    profile: Profile,
    cache: &TemplateCache,
    lot: &crate::synthetic::SyntheticLot,
    kind: CircuitKind,
    setup_ms: f64,
) -> MetricRow {
    let template = cache.get(kind);
    match prove_and_verify(template, lot, seed, profile) {
        Ok(stats) => ok_row(
            seed,
            events,
            template,
            stats.prove_ms,
            stats.verify_ms,
            stats.proof_bytes,
            setup_ms,
            stats.witness_ms,
            0.0,
        ),
        Err(err) => error_row(
            seed,
            events,
            kind,
            format!("prove_or_verify_failed:{err:#}"),
        ),
    }
}

fn run_wrapper_row(
    seed: usize,
    events: usize,
    profile: Profile,
    cache: &TemplateCache,
    lot: &crate::synthetic::SyntheticLot,
    setup_ms: f64,
) -> MetricRow {
    let wrapper = cache.get(CircuitKind::Wrapper);
    #[cfg(feature = "recursion")]
    {
        return match prove_recursive_wrapper(lot, seed, events, profile) {
            Ok(stats) => ok_row(
                seed,
                events,
                wrapper,
                stats.prove_ms,
                stats.verify_ms,
                stats.proof_bytes,
                setup_ms,
                stats.witness_ms,
                stats.inner_prove_ms,
            ),
            Err(err) => error_row(
                seed,
                events,
                CircuitKind::Wrapper,
                format!("recursive_prove_or_verify_failed:{err:#}"),
            ),
        };
    }

    #[cfg(not(feature = "recursion"))]
    {
        let _ = setup_ms;
        let inner_kinds = [
            CircuitKind::C2,
            CircuitKind::C3,
            CircuitKind::C4,
            CircuitKind::C5,
        ];
        let mut inner_proofs = Vec::with_capacity(inner_kinds.len());
        let mut inner_prove_ms = 0.0;
        for kind in inner_kinds {
            match prove_inner(cache.get(kind), lot, seed, profile) {
                Ok(bundle) => {
                    inner_prove_ms += bundle.prove_ms;
                    inner_proofs.push(bundle.proof_bytes);
                }
                Err(err) => {
                    return error_row(
                        seed,
                        events,
                        CircuitKind::Wrapper,
                        format!("inner_proof_failed:{kind:?}:{err:#}"),
                    );
                }
            }
        }

        let _ = (wrapper, inner_proofs, inner_prove_ms);
        error_row(
            seed,
            events,
            CircuitKind::Wrapper,
            "recursion_blocked:build without recursion feature; no mocked recursive proof emitted"
                .to_string(),
        )
    }
}

fn ok_row(
    seed: usize,
    events: usize,
    template: &crate::templates::CircuitTemplate,
    prove_ms: f64,
    verify_ms: f64,
    proof_bytes: usize,
    setup_ms: f64,
    witness_ms: f64,
    inner_prove_ms: f64,
) -> MetricRow {
    MetricRow {
        seed,
        events_per_lot: events,
        circuit: template.kind.as_str().to_string(),
        prove_ms,
        verify_ms,
        proof_bytes,
        peak_rss_mb: peak_rss_mb(),
        status: "ok".to_string(),
        note: template.note.to_string(),
        circuit_version: template.kind.version().to_string(),
        build_ms: template.build_ms,
        witness_ms,
        setup_ms,
        gate_count: template.gate_count,
        public_inputs: template.public_inputs,
        inner_prove_ms,
    }
}

fn error_row(seed: usize, events: usize, kind: CircuitKind, note: String) -> MetricRow {
    MetricRow {
        seed,
        events_per_lot: events,
        circuit: kind.as_str().to_string(),
        prove_ms: 0.0,
        verify_ms: 0.0,
        proof_bytes: 0,
        peak_rss_mb: peak_rss_mb(),
        status: "error".to_string(),
        note,
        circuit_version: kind.version().to_string(),
        build_ms: 0.0,
        witness_ms: 0.0,
        setup_ms: 0.0,
        gate_count: 0,
        public_inputs: 0,
        inner_prove_ms: 0.0,
    }
}

fn peak_rss_mb() -> f64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(kb) = status.lines().find_map(|line| {
                line.strip_prefix("VmPeak:").and_then(|rest| {
                    rest.split_whitespace()
                        .next()
                        .and_then(|raw| raw.parse::<f64>().ok())
                })
            }) {
                return kb / 1024.0;
            }
        }
    }

    unsafe {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return 0.0;
        }
        let usage = usage.assume_init();
        #[cfg(target_os = "macos")]
        {
            usage.ru_maxrss as f64 / (1024.0 * 1024.0)
        }
        #[cfg(not(target_os = "macos"))]
        {
            usage.ru_maxrss as f64 / 1024.0
        }
    }
}

#[cfg(all(test, not(feature = "recursion")))]
mod tests {
    use super::*;

    #[test]
    fn wrapper_without_recursion_feature_emits_blocker_row() -> Result<()> {
        let out = std::env::temp_dir().join(format!(
            "e1_wrapper_without_recursion_feature_{}.csv",
            std::process::id()
        ));
        run_benchmark(BenchmarkOptions {
            out: out.clone(),
            events_per_lot: vec![8],
            seeds: 1,
            circuits: vec![CircuitKind::Wrapper],
            profile: Profile::CoffeeSmall,
            jobs: 1,
            include_placeholders: false,
            strict_output: false,
        })?;

        let mut rows = csv::Reader::from_path(out)?
            .deserialize::<MetricRow>()
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 1);
        let row = rows.pop().expect("one wrapper row");
        assert_eq!(row.circuit, CircuitKind::Wrapper.as_str());
        assert_eq!(row.status, "error");
        assert_eq!(row.proof_bytes, 0);
        assert!(row.note.contains("no mocked recursive proof emitted"));
        Ok(())
    }
}
