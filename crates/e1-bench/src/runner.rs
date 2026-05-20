use crate::synthetic::synthetic_lot;
use crate::templates::{prove_inner, witness_for, wrapper_witness, TemplateCache};
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
            writer.serialize(row)?;
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
    match witness_for(template, lot, seed, profile).and_then(|bundle| {
        let prove_start = Instant::now();
        let proof = template.data.prove(bundle.witness)?;
        let prove_ms = prove_start.elapsed().as_secs_f64() * 1000.0;
        let compressed = template.data.compress(proof.clone())?;
        let proof_bytes = compressed.to_bytes().len();
        let verify_start = Instant::now();
        template.data.verify(proof)?;
        let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
        Ok((bundle.witness_ms, prove_ms, verify_ms, proof_bytes))
    }) {
        Ok((witness_ms, prove_ms, verify_ms, proof_bytes)) => ok_row(
            seed,
            events,
            template,
            prove_ms,
            verify_ms,
            proof_bytes,
            setup_ms,
            witness_ms,
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
    let inner_kinds = [
        CircuitKind::C1,
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
                inner_proofs.push(bundle.proof);
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

    match wrapper_witness(wrapper, &inner_proofs).and_then(|bundle| {
        let prove_start = Instant::now();
        let proof = wrapper.data.prove(bundle.witness)?;
        let prove_ms = prove_start.elapsed().as_secs_f64() * 1000.0;
        let compressed = wrapper.data.compress(proof.clone())?;
        let proof_bytes = compressed.to_bytes().len();
        let verify_start = Instant::now();
        wrapper.data.verify(proof)?;
        let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
        Ok((bundle.witness_ms, prove_ms, verify_ms, proof_bytes))
    }) {
        Ok((witness_ms, prove_ms, verify_ms, proof_bytes)) => ok_row(
            seed,
            events,
            wrapper,
            prove_ms,
            verify_ms,
            proof_bytes,
            setup_ms,
            witness_ms,
            inner_prove_ms,
        ),
        Err(err) => error_row(
            seed,
            events,
            CircuitKind::Wrapper,
            format!("wrapper_prove_or_verify_failed:{err:#}"),
        ),
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
