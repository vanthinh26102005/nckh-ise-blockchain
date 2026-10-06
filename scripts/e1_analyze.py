#!/usr/bin/env python3
import argparse
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

BASE_CIRCUITS = [
    "c1_polygon_outside",
    "c2_poseidon2_merkle_depth16",
    "c3_threshold_time",
    "c4_poseidon2_actor_authorization",
    "c5_poseidon2_sparse_nullifier_depth32",
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


def read_raw(path):
    df = pd.read_csv(path)
    if set(STRICT_COLUMNS).issubset(df.columns):
        return df[STRICT_COLUMNS].copy(), True

    required = {
        "circuit",
        "events_per_lot",
        "seed",
        "prove_ms",
        "verify_ms",
        "proof_bytes",
        "peak_rss_mb",
    }
    missing = required - set(df.columns)
    if missing:
        raise SystemExit(f"Raw CSV missing required columns: {sorted(missing)}")
    if "status" in df.columns:
        df = df[df["status"] == "ok"].copy()
    strict = pd.DataFrame(
        {
            "circuit": df["circuit"],
            "events_per_lot": df["events_per_lot"],
            "seed": df["seed"],
            "prove_time_s": df["prove_ms"].astype(float) / 1000.0,
            "verify_time_ms": df["verify_ms"].astype(float),
            "proof_size_bytes": df["proof_bytes"].astype(float),
            "peak_ram_gb": df["peak_rss_mb"].astype(float) / 1024.0,
        }
    )
    return strict, False


def write_table(df, out_path):
    metrics = ["prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb"]
    rows = []
    for (circuit, events), group in df.groupby(["circuit", "events_per_lot"], sort=True):
        row = {"circuit": circuit, "events_per_lot": int(events), "n": int(len(group))}
        for metric in metrics:
            mean, low, high = bootstrap_ci(group[metric].to_numpy())
            row[f"{metric}_median"] = float(group[metric].median())
            row[f"{metric}_q25"] = float(group[metric].quantile(0.25))
            row[f"{metric}_q75"] = float(group[metric].quantile(0.75))
            row[f"{metric}_mean"] = mean
            row[f"{metric}_ci95_low"] = low
            row[f"{metric}_ci95_high"] = high
        rows.append(row)
    pd.DataFrame(rows).sort_values(["circuit", "events_per_lot"]).to_csv(out_path, index=False)


def write_plots(df, out_path):
    import matplotlib.pyplot as plt

    metrics = ["prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb"]
    titles = ["Prove Time (s)", "Verify Time (ms)", "Proof Size (bytes)", "Process Peak RSS (GiB)"]
    fig, axes = plt.subplots(1, 4, figsize=(18, 4.8))

    for ax, metric, title in zip(axes, metrics, titles):
        for circuit, group in df.groupby("circuit", sort=True):
            series = group.groupby("events_per_lot")[metric].mean().sort_index()
            ax.plot(series.index, series.values, marker="o", linewidth=1.8, label=circuit)
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
        return "linux_proc_status_vmhwm_with_getrusage_fallback"
    return "getrusage_ru_maxrss"


def write_metadata(df, raw_path, out_path, command):
    metadata = {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "command": command,
        "raw_csv": raw_path,
        "rows": int(len(df)),
        "circuits": sorted(df["circuit"].unique().tolist()),
        "events_per_lot": sorted(int(v) for v in df["events_per_lot"].unique()),
        "seeds_per_cell": {
            f"{circuit}:{int(events)}": int(len(group["seed"].unique()))
            for (circuit, events), group in df.groupby(["circuit", "events_per_lot"])
        },
        "rustc": command_output(["rustc", "--version"]),
        "cargo": command_output(["cargo", "--version"]),
        "backend": "Plonky3 core",
        "hash": "Poseidon2",
        "plonky3_core_crates": {
            "source": "crates.io; exact package versions and checksums are in Cargo.lock",
            "manifest_version_requirement": "0.6",
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
        "ram_scope": "Process lifetime RSS high-water mark including template/setup memory; not isolated per circuit or seed",
        "ram_unit": "GiB (legacy column name peak_ram_gb)",
        "timing_scope": "Base proving and verification; witness generation and reusable template setup are outside prove_time_s",
        "git_commit": command_output(["git", "rev-parse", "HEAD"]),
        "git_status": command_output(["git", "status", "--short"]),
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
        f"- Rows: {len(df)}",
        f"- Circuits: {', '.join(sorted(df['circuit'].unique()))}",
        "- Prove time excludes witness generation and reusable template setup.",
        "- RAM is process lifetime peak RSS in GiB, including setup; it is not isolated per circuit/seed.",
        "- Proof size is measured from Plonky3 STARK proofs. The 196B target is not claimed.",
        "- C4 is a Poseidon2 actor authorization proof, not Ed25519/EdDSA production verification.",
        "- Ed25519/EdDSA remains a separate blocker.",
        "- Recursive-wrapper timings are separate measurements and are not included in a C1-C5 base grid.",
        "",
        "| Circuit | Events/lot | Seeds passed | Median prove s | Median verify ms | Mean proof bytes |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for (circuit, events), group in df.groupby(["circuit", "events_per_lot"], sort=True):
        proof = float(group["proof_size_bytes"].mean())
        lines.append(
            f"| {circuit} | {events} | {len(group)} | {group['prove_time_s'].median():.6f} | "
            f"{group['verify_time_ms'].median():.3f} | {proof:.1f} |"
        )
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")


def validate_grid(df, expected_seeds, expected_events=None, expected_circuits=None):
    if df.empty:
        raise SystemExit("Empty benchmark grid")
    keys = ["circuit", "events_per_lot", "seed"]
    if df[keys].isna().any().any():
        raise SystemExit("Missing circuit/events/seed key")
    if df.duplicated(keys).any():
        raise SystemExit("Duplicate circuit/events/seed keys")
    numeric = df[STRICT_COLUMNS[1:]].apply(pd.to_numeric, errors="coerce")
    if not np.isfinite(numeric.to_numpy(dtype=float)).all():
        raise SystemExit("Non-finite or non-numeric benchmark value")
    if (numeric <= 0).any().any():
        raise SystemExit("Benchmark values must be positive")
    for column in ["events_per_lot", "seed", "proof_size_bytes"]:
        if (numeric[column] % 1 != 0).any():
            raise SystemExit(f"Non-integer {column}")
    events_grid = set(expected_events) if expected_events is not None else set(df["events_per_lot"])
    circuits_grid = set(expected_circuits) if expected_circuits is not None else set(df["circuit"])
    expected_pairs = {(c, e) for c in circuits_grid for e in events_grid}
    actual_pairs = set(zip(df["circuit"], df["events_per_lot"]))
    if actual_pairs != expected_pairs:
        raise SystemExit(f"Invalid grid: missing={sorted(expected_pairs - actual_pairs)}, unexpected={sorted(actual_pairs - expected_pairs)}")
    bad = []
    for (circuit, events), group in df.groupby(["circuit", "events_per_lot"]):
        seeds = set(group["seed"])
        if expected_seeds is not None and seeds != set(range(1, expected_seeds + 1)):
            bad.append(f"{circuit}/{events}: seeds={sorted(seeds)}")
    if bad:
        raise SystemExit("Invalid seed count per cell: " + "; ".join(bad))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", required=True)
    parser.add_argument("--out-dir", default=None)
    parser.add_argument("--table-out", default=None)
    parser.add_argument("--plots-out", default=None)
    parser.add_argument("--metadata-out", default=None)
    parser.add_argument("--report-out", default=None)
    parser.add_argument("--expected-seeds", type=int, default=None)
    parser.add_argument("--expected-events", help="Exact CSV list of events/lot values")
    parser.add_argument("--base-grid", action="store_true", help="Require exactly the current C1-C5 circuits")
    parser.add_argument("--command", default="")
    args = parser.parse_args()

    df, _ = read_raw(args.raw)
    validate_grid(df, args.expected_seeds,
                  [int(v) for v in args.expected_events.split(",")] if args.expected_events else None,
                  BASE_CIRCUITS if args.base_grid else None)

    out_dir = args.out_dir or os.path.dirname(args.raw) or "."
    os.makedirs(out_dir, exist_ok=True)
    table_out = args.table_out or os.path.join(out_dir, "summary.csv")
    plots_out = args.plots_out or os.path.join(out_dir, "e1_plots.png")
    metadata_out = args.metadata_out or os.path.join(out_dir, "e1_metadata.json")
    report_out = args.report_out or os.path.join(out_dir, "report.md")

    write_table(df, table_out)
    write_plots(df, plots_out)
    write_metadata(df, args.raw, metadata_out, args.command)
    write_report(df, report_out)


if __name__ == "__main__":
    main()
