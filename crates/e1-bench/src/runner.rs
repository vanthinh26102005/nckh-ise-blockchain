use crate::synthetic::synthetic_lot;
#[cfg(feature = "recursion")]
use crate::templates::prove_recursive_wrapper_with_depth;
use crate::templates::{prove_and_verify, TemplateCache};
use crate::types::{
    BenchmarkOptions, CircuitKind, MetricRow, Profile, FRI_LOG_BLOWUP, FRI_NUM_QUERIES,
    FRI_QUERY_POW_BITS,
};
use anyhow::{bail, Result};
use rayon::prelude::*;
use std::fs;
use std::time::Instant;

pub fn run_benchmark(options: BenchmarkOptions) -> Result<()> {
    if ![6, 16].contains(&options.credential_depth) {
        bail!("--credential-depth must be 6 or 16");
    }
    if options.seeds == 0 || options.events_per_lot.is_empty() || options.circuits.is_empty() {
        bail!("benchmark grid must be nonempty");
    }
    if options.jobs == 0 {
        bail!("--jobs must be >= 1");
    }
    let mut writer = csv::Writer::from_writer(open_raw_output(&options.out)?);
    if options.strict_output {
        writer.write_record([
            "circuit",
            "events_per_lot",
            "seed",
            "prove_time_s",
            "verify_time_ms",
            "proof_size_bytes",
            "peak_ram_gb",
            "status",
            "note",
            "credential_depth",
            "circuit_version",
            "semantic_scope",
            "hash",
            "fri_log_blowup",
            "fri_num_queries",
            "fri_query_pow_bits",
            "security_model",
        ])?;
    }
    let mut failures = 0;
    for &events in &options.events_per_lot {
        let setup_start = Instant::now();
        let cache = TemplateCache::build_with_depth(
            events,
            options.profile,
            &options.circuits,
            options.credential_depth,
        )?;
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
            failures += usize::from(row.status != "ok");
            if options.strict_output {
                writer.write_record([
                    row.circuit,
                    row.events_per_lot.to_string(),
                    row.seed.to_string(),
                    row.prove_ms
                        .map(|v| format!("{:.9}", v / 1000.0))
                        .unwrap_or_default(),
                    row.verify_ms.map(|v| format!("{v:.6}")).unwrap_or_default(),
                    row.proof_bytes.map(|v| v.to_string()).unwrap_or_default(),
                    row.peak_rss_mb
                        .map(|v| format!("{:.9}", v / 1024.0))
                        .unwrap_or_default(),
                    row.status,
                    row.note,
                    row.credential_depth.to_string(),
                    row.circuit_version,
                    row.semantic_scope,
                    row.hash,
                    row.fri_log_blowup.to_string(),
                    row.fri_num_queries.to_string(),
                    row.fri_query_pow_bits.to_string(),
                    row.security_model,
                ])?;
            } else {
                writer.serialize(row)?;
            }
            writer.flush()?;
        }
    }
    if options.strict_output && failures > 0 {
        bail!(
            "strict benchmark has {failures} failed rows; all rows retained in {}",
            options.out.display()
        );
    }
    Ok(())
}

fn open_raw_output(out: &std::path::Path) -> Result<fs::File> {
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(||std::path::Path::new("."));
    fs::create_dir_all(parent)?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().canonicalize()?;
    if parent.canonicalize()?.starts_with(root) { bail!("raw benchmark output must be outside Git"); }
    Ok(fs::OpenOptions::new().write(true).create_new(true).open(out)?)
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
        .map(|&kind| {
            let mut row = match kind {
                CircuitKind::Wrapper => {
                    run_wrapper_row(seed, events, profile, cache, &lot, setup_ms)
                }
                _ => run_regular_row(seed, events, profile, cache, &lot, kind, setup_ms),
            };
            row.credential_depth = cache.get(kind).credential_depth;
            row
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
        return match prove_recursive_wrapper_with_depth(
            lot,
            seed,
            events,
            profile,
            wrapper.credential_depth,
        ) {
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
        let _ = (wrapper, lot, profile);
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
        credential_depth: 16,
        semantic_scope: "Path B circuit-cost research; C2 membership, C4 actor preimage, C5 32-bit Poseidon2 index; not equivalent to Path A SHA-256/256-bit nullifiers".to_string(),
        hash: "Poseidon2/Goldilocks (one-field statement digests)".to_string(),
        fri_log_blowup: FRI_LOG_BLOWUP,
        fri_num_queries: FRI_NUM_QUERIES,
        fri_query_pow_bits: FRI_QUERY_POW_BITS,
        security_model: "approximately 115 conjectured bits (3*33+16) checklist model; not a security certification".to_string(),
        seed,
        events_per_lot: events,
        circuit: template.kind.as_str().to_string(),
        prove_ms: Some(prove_ms),
        verify_ms: Some(verify_ms),
        proof_bytes: Some(proof_bytes),
        peak_rss_mb: peak_rss_mb(),
        status: "ok".to_string(),
        note: template.note.to_string(),
        circuit_version: template.kind.version().to_string(),
        build_ms: Some(template.build_ms),
        witness_ms: Some(witness_ms),
        setup_ms: Some(setup_ms),
        gate_count: None,
        public_inputs: template.public_inputs,
        inner_prove_ms: (template.kind == CircuitKind::Wrapper).then_some(inner_prove_ms),
    }
}

fn error_row(seed: usize, events: usize, kind: CircuitKind, note: String) -> MetricRow {
    MetricRow {
        credential_depth: 16,
        semantic_scope: "Path B circuit-cost research; C2 membership, C4 actor preimage, C5 32-bit Poseidon2 index; not equivalent to Path A SHA-256/256-bit nullifiers".to_string(),
        hash: "Poseidon2/Goldilocks (one-field statement digests)".to_string(),
        fri_log_blowup: FRI_LOG_BLOWUP,
        fri_num_queries: FRI_NUM_QUERIES,
        fri_query_pow_bits: FRI_QUERY_POW_BITS,
        security_model: "approximately 115 conjectured bits (3*33+16) checklist model; not a security certification".to_string(),
        seed,
        events_per_lot: events,
        circuit: kind.as_str().to_string(),
        prove_ms: None,
        verify_ms: None,
        proof_bytes: None,
        peak_rss_mb: peak_rss_mb(),
        status: "error".to_string(),
        note,
        circuit_version: kind.version().to_string(),
        build_ms: None,
        witness_ms: None,
        setup_ms: None,
        gate_count: None,
        public_inputs: 0,
        inner_prove_ms: None,
    }
}

fn peak_rss_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(kb) = status.lines().find_map(|line| {
                line.strip_prefix("VmHWM:").and_then(|rest| {
                    rest.split_whitespace()
                        .next()
                        .and_then(|raw| raw.parse::<f64>().ok())
                })
            }) {
                return Some(kb / 1024.0);
            }
        }
    }

    unsafe {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return None;
        }
        let usage = usage.assume_init();
        #[cfg(target_os = "macos")]
        {
            Some(usage.ru_maxrss as f64 / (1024.0 * 1024.0))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Some(usage.ru_maxrss as f64 / 1024.0)
        }
    }
}

#[cfg(all(test, not(feature = "recursion")))]
mod tests {
    use super::*;

    #[test]
    fn raw_output_rejects_git_paths_and_existing_evidence() -> Result<()> {
        assert!(open_raw_output(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).is_err());
        let out=std::env::temp_dir().join(format!("e1-output-boundary-{}-{}.csv",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()));
        drop(open_raw_output(&out)?);
        assert!(open_raw_output(&out).is_err());
        fs::remove_file(out)?;
        Ok(())
    }

    #[test]
    fn wrapper_without_recursion_feature_emits_blocker_row() -> Result<()> {
        let out = std::env::temp_dir().join(format!(
            "e1_wrapper_without_recursion_feature_{}.csv",
            std::process::id()
        ));
        run_benchmark(BenchmarkOptions {
            out: out.clone(),
            credential_depth: 6,
            events_per_lot: vec![8],
            seeds: 1,
            circuits: vec![CircuitKind::Wrapper],
            profile: Profile::CoffeeSmall,
            jobs: 1,
            include_placeholders: false,
            strict_output: false,
        })?;

        let mut rows = csv::Reader::from_path(&out)?
            .deserialize::<MetricRow>()
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 1);
        let row = rows.pop().expect("one wrapper row");
        assert_eq!(row.circuit, CircuitKind::Wrapper.as_str());
        assert_eq!(row.status, "error");
        assert_eq!(row.proof_bytes, None);
        assert!(row.note.contains("no mocked recursive proof emitted"));
        fs::remove_file(out)?;
        Ok(())
    }
}
