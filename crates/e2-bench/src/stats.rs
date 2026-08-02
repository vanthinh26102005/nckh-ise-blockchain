use crate::types::{E2Options, LatencyRow, LatencySummary};
use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

pub fn compute_summary(rows: &[LatencyRow], options: &E2Options) -> LatencySummary {
    let ok_rows: Vec<&LatencyRow> = rows.iter().filter(|r| r.status == "ok").collect();

    if ok_rows.is_empty() {
        return LatencySummary {
            profile: options.profile.to_string(),
            l1_mode: options.l1_mode.to_string(),
            lambda_events_per_min: options.lambda_events_per_min,
            duration_min: options.duration_min,
            seeds: options.seeds,
            total_events: 0,
            total_lots: 0,
            median_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            mean_ms: 0.0,
        };
    }

    let mut total_latencies: Vec<f64> = ok_rows
        .iter()
        .map(|r| r.total_latency_ms as f64)
        .collect();
    total_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let unique_lots: HashSet<(usize, usize)> =
        ok_rows.iter().map(|r| (r.seed, r.lot_id)).collect();

    let total_events = ok_rows.len();
    let total_lots = unique_lots.len();

    let min_ms = *total_latencies.first().unwrap_or(&0.0);
    let max_ms = *total_latencies.last().unwrap_or(&0.0);
    let sum_ms: f64 = total_latencies.iter().sum();
    let mean_ms = sum_ms / total_events as f64;

    let median_ms = percentile(&total_latencies, 50.0);
    let p95_ms = percentile(&total_latencies, 95.0);
    let p99_ms = percentile(&total_latencies, 99.0);

    LatencySummary {
        profile: options.profile.to_string(),
        l1_mode: options.l1_mode.to_string(),
        lambda_events_per_min: options.lambda_events_per_min,
        duration_min: options.duration_min,
        seeds: options.seeds,
        total_events,
        total_lots,
        median_ms,
        p95_ms,
        p99_ms,
        min_ms,
        max_ms,
        mean_ms,
    }
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let fract = rank - lower as f64;
    if lower == upper {
        sorted[lower]
    } else {
        sorted[lower] + fract * (sorted[upper] - sorted[lower])
    }
}

pub fn write_raw_csv(rows: &[LatencyRow], path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = csv::Writer::from_path(path)?;
    for row in rows {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(())
}

pub fn write_summary_csv(summary: &LatencySummary, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = csv::Writer::from_path(path)?;
    writer.serialize(summary)?;
    writer.flush()?;
    Ok(())
}

pub fn write_metadata_json(
    summary: &LatencySummary,
    options: &E2Options,
    path: &Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let metadata = json!({
        "timestamp_utc": Utc::now().to_rfc3339(),
        "experiment": "E2 - End-to-end latency",
        "profile": options.profile.to_string(),
        "l1_mode": options.l1_mode.to_string(),
        "lambda_events_per_min": options.lambda_events_per_min,
        "duration_min": options.duration_min,
        "seeds": options.seeds,
        "total_events": summary.total_events,
        "total_lots": summary.total_lots,
        "metrics_summary_ms": {
            "median": summary.median_ms,
            "p95": summary.p95_ms,
            "p99": summary.p99_ms,
            "min": summary.min_ms,
            "max": summary.max_ms,
            "mean": summary.mean_ms,
        },
        "pipeline_disclaimer": "Base proof pipeline latency using E1 Plonky3 base proofs (C2-C5). Full recursive ZK-Rollup aggregation remains blocked upstream.",
        "c4_status": "Poseidon2 actor authorization proof; Ed25519/EdDSA production verification remains a separate blocker",
        "recursion_status": "GitHub Plonky3-recursion rev 524665d is pinned, but wrapper aggregation currently panics with 'trace_next is always present'; no mocked recursive proof is emitted",
        "system": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        }
    });

    let mut file = File::create(path)?;
    file.write_all(serde_json::to_string_pretty(&metadata)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::L1Mode;

    #[test]
    fn test_percentile_calculation() {
        let data = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile(&data, 0.0), 10.0);
        assert_eq!(percentile(&data, 50.0), 30.0);
        assert_eq!(percentile(&data, 100.0), 50.0);
    }
}
