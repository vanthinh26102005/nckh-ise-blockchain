#!/usr/bin/env python3
import argparse
import hashlib
import itertools
import json
import os
import platform
import subprocess
from datetime import datetime, timezone

import numpy as np
import pandas as pd


STRICT_COLUMNS = [
    "circuit",
    "events_per_lot",
    "seed",
    "prove_time_s",
    "verify_time_ms",
    "proof_size_bytes",
    "peak_ram_gb",
]


def bootstrap_ci(values, resamples=10_000, seed=20260521):
    arr = np.asarray(values, dtype=float)
    if arr.size == 0:
        return np.nan, np.nan, np.nan
    if arr.size == 1:
        mean = float(arr[0])
        return mean, mean, mean
    rng = np.random.default_rng(seed)
    samples = rng.choice(arr, size=(resamples, arr.size), replace=True)
    means = samples.mean(axis=1)
    return float(arr.mean()), float(np.percentile(means, 2.5)), float(np.percentile(means, 97.5))


METRICS = ["prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb"]
CELL = ["circuit", "events_per_lot", "credential_depth"]
PROVENANCE = ["circuit_version", "semantic_scope", "hash", "fri_log_blowup", "fri_num_queries", "fri_query_pow_bits", "security_model"]
VERSIONS = {
    "c1_polygon_outside": "v4-plonky3-wgs84-polygon-poseidon2-commitments",
    "c2_poseidon2_merkle": "v5-plonky3-poseidon2-merkle-selectable-depth",
    "c3_threshold_time": "v4-plonky3-threshold-time",
    "c4_poseidon2_actor_authorization": "v4-plonky3-poseidon2-actor-authorization",
    "c5_poseidon2_sparse_nullifier_depth32": "v5-plonky3-poseidon2-sparse-nullifier-depth32",
    "wrapper_recursive_plonky3": "v5-plonky3-recursion-github-blocked",
}
CURRENT_PROVENANCE = {
    "semantic_scope": "Path B circuit-cost research; C2 membership, C4 actor preimage, C5 32-bit Poseidon2 index; not equivalent to Path A SHA-256/256-bit nullifiers",
    "hash": "Poseidon2/Goldilocks (one-field statement digests)",
    "security_model": "approximately 115 conjectured bits (3*33+16) checklist model; not a security certification",
}


def read_raw(path):
    df = pd.read_csv(path)
    strict = set(STRICT_COLUMNS).issubset(df.columns)
    required = set(STRICT_COLUMNS) if strict else {"circuit", "events_per_lot", "seed", "prove_ms", "verify_ms", "proof_bytes", "peak_rss_mb"}
    missing = required - set(df.columns)
    if missing:
        raise SystemExit(f"Raw CSV missing required columns: {sorted(missing)}")
    if not strict:
        for target, source, divisor in [("prove_time_s", "prove_ms", 1000), ("verify_time_ms", "verify_ms", 1), ("proof_size_bytes", "proof_bytes", 1), ("peak_ram_gb", "peak_rss_mb", 1024)]:
            df[target] = pd.to_numeric(df[source], errors="coerce") / divisor
    for metric in METRICS:
        df[metric] = pd.to_numeric(df[metric], errors="coerce")
    if "status" not in df:
        df["status"] = "legacy_unverified"
    if "credential_depth" not in df:
        df["credential_depth"] = "unknown"
    df["source_row"] = np.arange(2, len(df) + 2)
    return df, strict


def successful(df):
    # Legacy metrics can be described, but never accepted as strict provenance.
    return df[df["status"].isin(["ok", "legacy_unverified"])].copy()


def write_table(df, out_path):
    metrics = ["prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb"]
    rows = []
    for (circuit, events, depth), group in successful(df).groupby(CELL, sort=True):
        row = {"circuit": circuit, "events_per_lot": int(events), "credential_depth": depth, "n": int(len(group))}
        for metric in metrics:
            mean, low, high = bootstrap_ci(pd.to_numeric(group[metric], errors="coerce").dropna().to_numpy())
            row[f"{metric}_mean"] = mean
            row[f"{metric}_ci95_low"] = low
            row[f"{metric}_ci95_high"] = high
        rows.append(row)
    pd.DataFrame(rows).to_csv(out_path, index=False, na_rep="N/A")


def write_plots(df, out_path):
    import matplotlib.pyplot as plt

    metrics = ["prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb"]
    titles = ["Prove Time (s)", "Verify Time (ms)", "Proof Size (bytes)", "Peak RAM (GB)"]
    fig, axes = plt.subplots(1, 4, figsize=(18, 4.8))

    for ax, metric, title in zip(axes, metrics, titles):
        for (circuit, depth), group in successful(df).groupby(["circuit", "credential_depth"], sort=True):
            series = group.groupby("events_per_lot")[metric].mean().sort_index()
            ax.plot(series.index, series.values, marker="o", linewidth=1.8, label=f"{circuit}/depth{depth}")
        ax.set_title(title)
        ax.set_xlabel("Events per Lot")
        ax.grid(True, alpha=0.25)
    axes[-1].legend(fontsize=7, loc="best")
    fig.tight_layout()
    fig.savefig(out_path, dpi=150)
    plt.close(fig)


def command_output(cmd):
    try:
        return subprocess.check_output(cmd, stderr=subprocess.DEVNULL, text=True).strip()
    except Exception:
        return None


def ram_backend():
    if platform.system().lower() == "linux" and os.path.exists("/proc/self/status"):
        return "linux_proc_status_vmhwm"
    return "getrusage_ru_maxrss"


def write_metadata(df, raw_path, out_path, command):
    metadata = {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "command": command,
        "raw_csv": raw_path,
        "rows": int(len(df)),
        "raw_sha256": hashlib.sha256(open(raw_path, "rb").read()).hexdigest(),
        "status_counts": df["status"].value_counts().to_dict(),
        "failed_rows": df[~df["status"].isin(["ok", "legacy_unverified"])].fillna("N/A").to_dict("records"),
        "provenance": {key: df[key].dropna().astype(str).unique().tolist() if key in df else ["N/A"] for key in PROVENANCE},
        "credential_depths": df["credential_depth"].unique().tolist(),
        "security_checklist_model": "3*33+16 = approximately 115 conjectured bits; not a security certification; field/hash/protocol bounds apply",
        "semantic_scope": "Path B circuit-cost research; not equivalent to Path A SHA-256/256-bit nullifiers",
        "circuits": sorted(df["circuit"].unique().tolist()),
        "events_per_lot": sorted(int(v) for v in df["events_per_lot"].unique()),
        "seeds_per_cell": {
            f"{circuit}:{int(events)}:{depth}": int(len(group["seed"].unique()))
            for (circuit, events, depth), group in df.groupby(CELL)
        },
        "rustc": command_output(["rustc", "--version"]),
        "cargo": command_output(["cargo", "--version"]),
        "backend": "Plonky3 core",
        "hash": "Poseidon2",
        "plonky3_core_crates": {
            "source": "https://github.com/Plonky3/Plonky3",
            "version": "0.6 (exact resolved packages recorded in Cargo.lock)",
            "crates": [
                "p3-field",
                "p3-goldilocks",
                "p3-poseidon2",
                "p3-uni-stark",
                "p3-fri",
                "p3-merkle-tree",
            ],
        },
        "plonky3_recursion_crates": {
            "source": "https://github.com/Plonky3/Plonky3-recursion",
            "rev": "b36339709a7a67ee9760fb578b3d4339fd983709",
            "status": "research prototype dependency; active development and unaudited",
        },
        "python": platform.python_version(),
        "os": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "cpu_logical": os.cpu_count(),
        "ram_backend": ram_backend(),
        "proof_size_policy": "measured Plonky3 STARK proof bytes; 196 bytes is paper target, not claimed",
        "c4_status": "Poseidon2 actor authorization proof; Ed25519/EdDSA production verification remains a separate blocker",
        "recursion_status": "Plonky3-recursion revision b363397 has a real Rust wrapper prove/verify smoke test for C1-C5; Solidity/EVM verification is not implemented",
    }
    with open(out_path, "w") as f:
        json.dump(metadata, f, indent=2)
        f.write("\n")


def write_report(df, out_path):
    lines = [
        "# E1 Strict Benchmark Report",
        "",
        f"- Rows retained: {len(df)}; statuses: {df['status'].value_counts().to_dict()}",
        "- Path B measures circuit cost; C2 private event membership, C4 actor-preimage authorization, C5 32-bit Poseidon2 sparse index are not equivalent to Path A SHA-256/256-bit nullifiers.",
        "- FRI checklist: log blowup 3, 33 queries, 16 query grinding bits: approximately 115 conjectured bits; not a security certification.",
        "- Missing metrics are N/A. Failed rows and provenance are retained in metadata and analyzed_rows.csv.",
        f"- Circuits: {', '.join(sorted(df['circuit'].unique()))}",
        "- Proof size is measured from Plonky3 STARK proofs. The 196B target is not claimed.",
        "- C4 is a Poseidon2 actor authorization proof, not Ed25519/EdDSA production verification.",
        "- Ed25519/EdDSA remains a separate blocker.",
        "- Recursive aggregation uses Plonky3-recursion revision b363397 and has a real Rust C1-C5 prove/verify smoke test; Solidity/EVM verification is not implemented.",
        "",
        "| Circuit | Rows | Mean prove s | Mean verify ms | Mean proof bytes | Target gap bytes |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    def display(value, precision):
        return f"{value:.{precision}f}" if np.isfinite(value) else "N/A"

    for circuit, group in successful(df).groupby("circuit", sort=True):
        proof = float(group["proof_size_bytes"].mean())
        lines.append(
            f"| {circuit} | {len(group)} | {display(group['prove_time_s'].mean(), 6)} | "
            f"{display(group['verify_time_ms'].mean(), 3)} | {display(proof, 1)} | {display(proof - 196, 1)} |"
        )
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")


def validate_grid(df, expected_seeds, expected_circuits=None, expected_events=None, expected_depths=None, strict=False):
    bad = []
    if df.empty:
        bad.append("empty grid")
    keys = CELL + ["seed"]
    if df.duplicated(keys).any():
        bad.append("duplicate circuit/size/depth/seed rows")
    for column in ["seed", "events_per_lot"]:
        numbers = pd.to_numeric(df[column], errors="coerce")
        if numbers.isna().any() or ((numbers <= 0) | (numbers % 1 != 0)).any():
            bad.append(f"{column} must contain positive integers")
    observed = set(tuple(row) for row in df[CELL].itertuples(index=False, name=None))
    if all(v is not None for v in [expected_circuits, expected_events, expected_depths]):
        expected = set(itertools.product(expected_circuits, expected_events, expected_depths))
        if observed != expected:
            bad.append(f"grid coverage mismatch: missing={sorted(expected-observed, key=str)}, unexpected={sorted(observed-expected, key=str)}")
    elif strict:
        bad.append("strict validation requires expected circuits, events and depths")
    for cell, group in df.groupby(CELL):
        if expected_seeds is not None and set(group["seed"]) != set(range(1, expected_seeds + 1)):
            bad.append(f"{cell}: expected unique seeds 1..{expected_seeds}")
    if not df["status"].isin(["ok", "legacy_unverified"] if not strict else ["ok"]).all():
        bad.append("failed or unverified rows retained")
    if expected_seeds is not None and expected_seeds <= 0:
        bad.append("expected seeds must be positive")
    if strict and expected_seeds is None:
        bad.append("strict validation requires expected seeds")
    for metric in METRICS:
        if metric not in df:
            bad.append(f"missing metric {metric}")
            continue
        values = pd.to_numeric(df[metric], errors="coerce")
        if (~np.isfinite(values) | (values <= 0)).any():
            bad.append(f"missing/nonpositive/nonfinite {metric}")
    if strict:
        for column in PROVENANCE:
            if column not in df or df[column].isna().any():
                bad.append(f"missing provenance {column}")
        for column, expected in [("fri_log_blowup", 3), ("fri_num_queries", 33), ("fri_query_pow_bits", 16)]:
            if column in df and not (pd.to_numeric(df[column], errors="coerce") == expected).all():
                bad.append(f"unexpected security parameter {column}")
        for column, expected in CURRENT_PROVENANCE.items():
            if column in df and not (df[column] == expected).all():
                bad.append(f"provenance differs from current Path B implementation: {column}")
        if "circuit_version" in df:
            for circuit, group in df.groupby("circuit"):
                if circuit not in VERSIONS or not (group["circuit_version"] == VERSIONS.get(circuit)).all():
                    bad.append(f"unsupported/stale circuit version for {circuit}")
    if bad:
        raise SystemExit("Invalid E1 evidence: " + "; ".join(bad))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", required=True)
    parser.add_argument("--out-dir", default=None)
    parser.add_argument("--table-out", default=None)
    parser.add_argument("--plots-out", default=None)
    parser.add_argument("--metadata-out", default=None)
    parser.add_argument("--report-out", default=None)
    parser.add_argument("--expected-seeds", type=int, default=None)
    parser.add_argument("--command", default="")
    parser.add_argument("--expected-circuits", help="exact CSV circuit identifiers")
    parser.add_argument("--expected-events", help="CSV event counts")
    parser.add_argument("--expected-depths", help="CSV credential depths")
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--no-plots", action="store_true")
    args = parser.parse_args()

    df, _ = read_raw(args.raw)

    out_dir = args.out_dir or os.path.dirname(args.raw) or "."
    os.makedirs(out_dir, exist_ok=True)
    table_out = args.table_out or os.path.join(out_dir, "summary.csv")
    plots_out = args.plots_out or os.path.join(out_dir, "e1_plots.png")
    metadata_out = args.metadata_out or os.path.join(out_dir, "e1_metadata.json")
    report_out = args.report_out or os.path.join(out_dir, "report.md")

    write_table(df, table_out)
    if not args.no_plots:
        write_plots(df, plots_out)
    write_metadata(df, args.raw, metadata_out, args.command)
    write_report(df, report_out)
    df.to_csv(os.path.join(out_dir, "analyzed_rows.csv"), index=False, na_rep="N/A")
    validate_grid(df, args.expected_seeds, args.expected_circuits.split(",") if args.expected_circuits else None, [int(v) for v in args.expected_events.split(",")] if args.expected_events else None, [int(v) for v in args.expected_depths.split(",")] if args.expected_depths else None, args.strict)


if __name__ == "__main__":
    main()
