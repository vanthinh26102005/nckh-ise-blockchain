#!/usr/bin/env python3
"""Analyze paired E3 runs. Raw JSONL stays local; summaries are reproducible."""

import argparse
import json
from itertools import combinations
from pathlib import Path

import numpy as np
from scipy.stats import wilcoxon


BASELINES = ("fabric-only", "hash-on-chain", "vecro-token-adapted", "proposed")
METRICS = ("completionRate", "throughputShipmentsPerMin", "throughputEventsPerMin", "auditLatencyMs", "gasPerShipment", "calldataBytesPerShipment")


def load(path, baseline, expected_seeds, allow_pilot):
    records = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    run_records = [row for row in records if row.get("kind") == "run"]
    end_records = [row for row in records if row.get("kind") == "seed-end"]
    runs = {int(row["seed"]): row for row in run_records}
    endings = {int(row["seed"]): row for row in end_records}
    rows = [row for row in records if row.get("kind") == "shipment"]
    if (len(run_records) != expected_seeds or len(end_records) != expected_seeds
        or len(runs) != expected_seeds or len(endings) != expected_seeds
        or set(runs) != set(range(1, expected_seeds + 1))):
        raise ValueError(f"{baseline}: expected one complete record for each of {expected_seeds} seeds")
    if any(row.get("baseline") != baseline for row in records):
        raise ValueError(f"{baseline}: mixed baseline records")
    by_seed = {}
    for seed, run in runs.items():
        if run.get("mode") != "main" and not allow_pilot:
            raise ValueError(f"{baseline} seed {seed}: pilot data are not paper evidence")
        if run.get("mode") == "main" and (run.get("durationMin") != 60.0 or run.get("lambdaEventsPerMin") != 480.0
            or run.get("producerCount") != 125):
            raise ValueError(f"{baseline} seed {seed}: wrong main workload")
        subset = [row for row in rows if int(row["seed"]) == seed]
        ids = [row["shipmentId"] for row in subset]
        if len(ids) != len(set(ids)) or len(ids) != run["expectedShipments"]:
            raise ValueError(f"{baseline} seed {seed}: missing or duplicate shipment rows")
        if any(row.get("status") not in ("ok", "error", "timeout") for row in subset):
            raise ValueError(f"{baseline} seed {seed}: invalid status")
        if any(row.get("status") == "ok" and row.get("receipt") is None for row in subset):
            raise ValueError(f"{baseline} seed {seed}: success without readback/receipt")
        by_seed[seed] = (run, subset)
    return by_seed


def row_cost(row, field):
    value = row["receipt"].get(field)
    return float(value) if value is not None else None


def per_seed(run, rows):
    ok = [row for row in rows if row["status"] == "ok"]
    within_arrival = [row for row in ok if float(row["completedAtMs"]) <= float(run["durationMin"]) * 60_000]
    latencies = [float(row["auditLatencyMs"]) for row in ok]
    gas = [row_cost(row, "gasUsed") for row in ok]
    calldata = [row_cost(row, "calldataBytes") for row in ok]
    def mean_or_none(values):
        present = [value for value in values if value is not None]
        return float(np.mean(present)) if present else None
    return {
        "expectedShipments": len(rows),
        "completedShipments": len(ok),
        "completionRate": len(ok) / len(rows) if rows else None,
        "throughputShipmentsPerMin": len(within_arrival) / float(run["durationMin"]),
        "throughputEventsPerMin": sum(row["eventCount"] for row in within_arrival) / float(run["durationMin"]),
        "auditLatencyMs": float(np.median(latencies)) if latencies else None,
        "gasPerShipment": mean_or_none(gas),
        "calldataBytesPerShipment": mean_or_none(calldata),
    }


def describe(values, rng, iterations):
    values = np.asarray([value for value in values if value is not None], dtype=float)
    if not len(values):
        return None
    samples = rng.choice(values, size=(iterations, len(values)), replace=True)
    medians = np.median(samples, axis=1)
    return {
        "n": len(values), "median": float(np.median(values)),
        "iqr": [float(np.percentile(values, 25)), float(np.percentile(values, 75))],
        "p95": float(np.percentile(values, 95)),
        "bootstrapMedian95CI": [float(np.percentile(medians, 2.5)), float(np.percentile(medians, 97.5))],
    }


def shipment_distribution(rows, field):
    values = [float(value) for row in rows if row["status"] == "ok"
              for value in [row["auditLatencyMs"] if field == "auditLatencyMs"
                            else row_cost(row, field)] if value is not None]
    if not values:
        return None
    return {"n": len(values), "median": float(np.median(values)),
            "iqr": [float(np.percentile(values, 25)), float(np.percentile(values, 75))],
            "p95": float(np.percentile(values, 95))}


def compare(by_baseline):
    results = {}
    for metric in METRICS:
        pairs = []
        for first, second in combinations(BASELINES, 2):
            paired = [(by_baseline[first][seed][metric], by_baseline[second][seed][metric])
                      for seed in sorted(by_baseline[first])]
            paired = [(a, b) for a, b in paired if a is not None and b is not None]
            if not paired:
                continue
            left, right = zip(*paired)
            differences = np.asarray(left) - np.asarray(right)
            if np.all(differences == 0):
                p = 1.0
            else:
                p = float(wilcoxon(left, right, zero_method="wilcox", alternative="two-sided").pvalue)
            pairs.append({"first": first, "second": second, "pairedSeeds": len(paired),
                          "medianDifference": float(np.median(differences)), "pRaw": p})
        correction = len(pairs)
        for pair in pairs:
            pair["pBonferroni"] = min(1.0, pair["pRaw"] * correction)
            pair["comparisonCount"] = correction
        results[metric] = pairs
    return results


def analyze(paths, expected_seeds=30, allow_pilot=False, bootstrap_iterations=10_000):
    raw = {baseline: load(Path(paths[baseline]), baseline, expected_seeds, allow_pilot)
           for baseline in BASELINES}
    for seed in range(1, expected_seeds + 1):
        hashes = {raw[baseline][seed][0]["manifestSha256"] for baseline in BASELINES}
        counts = {raw[baseline][seed][0]["expectedShipments"] for baseline in BASELINES}
        if len(hashes) != 1 or len(counts) != 1:
            raise ValueError(f"seed {seed}: baselines did not replay the same canonical manifest")
        if not allow_pilot and {raw[baseline][seed][0]["mode"] for baseline in BASELINES} != {"main"}:
            raise ValueError(f"seed {seed}: mixed pilot/main runs")
    by_baseline = {baseline: {seed: per_seed(*raw[baseline][seed]) for seed in raw[baseline]}
                   for baseline in BASELINES}
    rng = np.random.default_rng(21)
    summary = {baseline: {metric: describe([by_baseline[baseline][seed][metric]
                 for seed in sorted(by_baseline[baseline])], rng, bootstrap_iterations)
                 for metric in METRICS} for baseline in BASELINES}
    shipment_level = {baseline: {metric: shipment_distribution(
        [row for seed in raw[baseline] for row in raw[baseline][seed][1]], field)
        for metric, field in (("auditLatencyMs", "auditLatencyMs"),
                              ("gasPerShipment", "gasUsed"),
                              ("calldataBytesPerShipment", "calldataBytes"))}
        for baseline in BASELINES}
    return {"experiment": "E3 four-baseline paired comparison",
            "evidence": "pilot" if allow_pilot else "main", "seeds": expected_seeds,
            "bootstrapIterations": bootstrap_iterations,
            "metadata": {baseline: {seed: raw[baseline][seed][0] for seed in raw[baseline]}
                         for baseline in BASELINES},
            "perSeed": by_baseline, "summary": summary,
            "shipmentDistributions": shipment_level,
            "pairedWilcoxonBonferroni": compare(by_baseline)}


def main():
    parser = argparse.ArgumentParser()
    for baseline in BASELINES:
        parser.add_argument("--" + baseline, type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expected-seeds", type=int, default=30)
    parser.add_argument("--bootstrap-iterations", type=int, default=10_000)
    parser.add_argument("--allow-pilot", action="store_true")
    args = parser.parse_args()
    result = analyze({baseline: getattr(args, baseline.replace("-", "_")) for baseline in BASELINES},
                     args.expected_seeds, args.allow_pilot, args.bootstrap_iterations)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
