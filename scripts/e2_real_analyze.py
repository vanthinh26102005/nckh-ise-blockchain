#!/usr/bin/env python3
"""Summarize a complete real E2 JSONL run; rejects debug-pilot data by default."""

import argparse
import json
from pathlib import Path
from statistics import fmean


def percentile(values, percentage):
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = percentage / 100 * (len(ordered) - 1)
    low, high = int(rank), min(int(rank) + 1, len(ordered) - 1)
    return ordered[low] + (ordered[high] - ordered[low]) * (rank - low)


def summarize(rows):
    latencies = [float(row["fabricAcknowledgementToReceiptMs"]) for row in rows]
    return {
        "lots": len(rows),
        "medianMs": percentile(latencies, 50),
        "iqrMs": [percentile(latencies, 25), percentile(latencies, 75)],
        "p95Ms": percentile(latencies, 95),
        "meanMs": fmean(latencies),
        "completionRate": 1.0,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expected-seeds", type=int, default=30)
    parser.add_argument("--allow-accelerated", action="store_true")
    args = parser.parse_args()

    records = [json.loads(line) for line in args.raw.read_text().splitlines() if line.strip()]
    runs = [record for record in records if record.get("kind") == "run"]
    lots = [record for record in records if record.get("kind") == "lot"]
    if len(runs) != 1 or not lots:
        raise SystemExit("raw JSONL must contain exactly one run record and at least one successful lot")
    run = runs[0]
    if run.get("mode") == "accelerated-pilot" and not args.allow_accelerated:
        raise SystemExit("accelerated pilot data is not valid E2 paper evidence")
    expected = set(range(1, args.expected_seeds + 1))
    actual = {int(lot["seed"]) for lot in lots}
    if actual != expected:
        raise SystemExit(f"expected exactly seeds {sorted(expected)}, found {sorted(actual)}")
    if any(lot.get("status") != "ok" for lot in lots):
        raise SystemExit("raw JSONL contains a non-successful lot")

    per_seed = {
        str(seed): summarize([lot for lot in lots if int(lot["seed"]) == seed])
        for seed in sorted(actual)
    }
    result = {
        "experiment": "E2 real Fabric acknowledgement to Anvil receipt",
        "run": run,
        "aggregate": summarize(lots),
        "perSeed": per_seed,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
