#!/usr/bin/env python3
import argparse
import json
import os
import platform
import subprocess
from datetime import datetime, timezone

import numpy as np
import pandas as pd


REQUIRED_COLUMNS = [
    "seed",
    "event_id",
    "lot_id",
    "epoch_id",
    "l1_mode",
    "ingest_at_ms",
    "lot_ready_at_ms",
    "proof_start_ms",
    "proof_end_ms",
    "submit_tx_at_ms",
    "confirmed_at_ms",
    "total_latency_ms",
    "status",
]


def read_raw(path):
    df = pd.read_csv(path)
    missing = set(REQUIRED_COLUMNS) - set(df.columns)
    if missing:
        raise SystemExit(f"Raw CSV missing required columns: {sorted(missing)}")
    if "status" in df.columns:
        df = df[df["status"] == "ok"].copy()
    return df


def write_cdf_plot(df, out_path):
    import matplotlib.pyplot as plt

    latencies = np.sort(df["total_latency_ms"].to_numpy(dtype=float))
    if len(latencies) == 0:
        print("Warning: empty latencies dataset, skipping plot generation.")
        return

    p = np.arange(1, len(latencies) + 1) / len(latencies)

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 5))

    # CDF plot
    ax1.plot(latencies, p, marker=".", linestyle="-", color="#1f77b4", linewidth=1.5, markersize=3)
    ax1.set_title("E2 End-to-End Latency CDF")
    ax1.set_xlabel("Total Latency (ms)")
    ax1.set_ylabel("Cumulative Probability")
    ax1.grid(True, alpha=0.3)

    median_val = np.median(latencies)
    p95_val = np.percentile(latencies, 95)
    p99_val = np.percentile(latencies, 99)

    ax1.axvline(median_val, color="green", linestyle="--", label=f"Median: {median_val:.1f} ms")
    ax1.axvline(p95_val, color="orange", linestyle="--", label=f"P95: {p95_val:.1f} ms")
    ax1.axvline(p99_val, color="red", linestyle="--", label=f"P99: {p99_val:.1f} ms")
    ax1.legend(loc="lower right")

    # Histogram
    ax2.hist(latencies, bins=30, color="#2ca02c", edgecolor="black", alpha=0.7)
    ax2.set_title("E2 End-to-End Latency Distribution")
    ax2.set_xlabel("Total Latency (ms)")
    ax2.set_ylabel("Event Count")
    ax2.grid(True, alpha=0.3)

    fig.tight_layout()
    fig.savefig(out_path, dpi=150)
    plt.close(fig)
    print(f"Saved plot: {out_path}")


def write_report(df, summary_path, report_path):
    latencies = df["total_latency_ms"].to_numpy(dtype=float)
    total_events = len(latencies)
    total_lots = len(df.groupby(["seed", "lot_id"]))
    seeds = len(df["seed"].unique())
    l1_mode = df["l1_mode"].iloc[0] if not df.empty else "unknown"

    median_val = float(np.median(latencies)) if total_events > 0 else 0.0
    p95_val = float(np.percentile(latencies, 95)) if total_events > 0 else 0.0
    p99_val = float(np.percentile(latencies, 99)) if total_events > 0 else 0.0
    min_val = float(np.min(latencies)) if total_events > 0 else 0.0
    max_val = float(np.max(latencies)) if total_events > 0 else 0.0
    mean_val = float(np.mean(latencies)) if total_events > 0 else 0.0

    report_lines = [
        "# E2 End-to-End Latency Benchmark Report",
        "",
        "## Configuration & Scope",
        f"- **L1 Mode**: `{l1_mode}`",
        f"- **Seeds**: `{seeds}`",
        f"- **Total Ingested Events**: `{total_events}`",
        f"- **Total Formed Lots**: `{total_lots}`",
        "- **Proof Components**: E1 Plonky3 base proofs (C2 Poseidon2 Merkle, C3 Threshold/Time, C4 Actor Auth, C5 Nullifier).",
        "- **Proof Timing Scope**: `proof_start_ms` → `proof_end_ms` uses measured E1 `witness_ms + prove_ms` cached by circuit and lot size; reusable template/setup time is outside the timed proof window.",
        "- **L1 Scope**: L1 confirmation is deterministic simulation. No Anvil/Sepolia RPC transaction is submitted by this benchmark.",
        "- **Disclaimer**: Reported numbers represent **base proof pipeline latency**. Full recursive rollup latency is not claimed as Plonky3 recursive wrapper remains blocked upstream.",
        "",
        "## Summary Metrics",
        "",
        "| Metric | Latency (ms) | Latency (s) |",
        "|---|---:|---:|",
        f"| **Median (P50)** | {median_val:.2f} | {median_val / 1000.0:.3f} |",
        f"| **P95** | {p95_val:.2f} | {p95_val / 1000.0:.3f} |",
        f"| **P99** | {p99_val:.2f} | {p99_val / 1000.0:.3f} |",
        f"| **Min** | {min_val:.2f} | {min_val / 1000.0:.3f} |",
        f"| **Max** | {max_val:.2f} | {max_val / 1000.0:.3f} |",
        f"| **Mean** | {mean_val:.2f} | {mean_val / 1000.0:.3f} |",
        "",
        "## Stage 3 RQ2 Context",
        "This E2 latency profile provides empirical baseline measurements for Paper 01 RQ2 under standard EPCIS workload.",
    ]

    with open(report_path, "w") as f:
        f.write("\n".join(report_lines) + "\n")
    print(f"Saved report: {report_path}")


def main():
    parser = argparse.ArgumentParser(description="E2 Latency Analysis & Plotter")
    parser.add_argument("--raw", required=True, help="Path to raw CSV file")
    parser.add_argument("--out-dir", default="results", help="Directory for output artifacts")
    parser.add_argument("--cdf-out", default=None, help="Path for CDF PNG plot")
    parser.add_argument("--report-out", default=None, help="Path for markdown report")
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    df = read_raw(args.raw)

    cdf_out = args.cdf_out or os.path.join(args.out_dir, "e2_latency_cdf.png")
    report_out = args.report_out or os.path.join(args.out_dir, "e2_latency_report.md")

    write_cdf_plot(df, cdf_out)
    write_report(df, args.raw, report_out)


if __name__ == "__main__":
    main()
