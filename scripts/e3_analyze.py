#!/usr/bin/env python3
"""Strict paired E3 summary; never turns partial/pilot runs into paper claims."""
import argparse
import itertools
import json
import random
import statistics
from collections import defaultdict
from pathlib import Path

from e3_run import MODES


def percentile(values, fraction):
    values = sorted(values)
    if not values:
        return None
    position = fraction * (len(values) - 1)
    lower = int(position)
    upper = min(lower + 1, len(values) - 1)
    return values[lower] + (values[upper] - values[lower]) * (position - lower)


def bootstrap_mean_ci(values, seed=0):
    rng = random.Random(seed)
    means = [statistics.fmean(rng.choices(values, k=len(values))) for _ in range(10_000)]
    return [percentile(means, 0.025), percentile(means, 0.975)]


def wilcoxon_exact(left, right):
    differences = [a - b for a, b in zip(left, right) if a != b]
    if not differences:
        return 1.0
    magnitudes = sorted((abs(d), index) for index, d in enumerate(differences))
    doubled_ranks = [0] * len(differences)
    cursor = 0
    while cursor < len(magnitudes):
        end = cursor + 1
        while end < len(magnitudes) and magnitudes[end][0] == magnitudes[cursor][0]:
            end += 1
        rank2 = (cursor + 1) + end
        for _, index in magnitudes[cursor:end]:
            doubled_ranks[index] = rank2
        cursor = end
    observed = sum(rank for rank, diff in zip(doubled_ranks, differences) if diff > 0)
    total = sum(doubled_ranks)
    distance = min(observed, total - observed)
    distribution = {0: 1}
    for rank in doubled_ranks:
        next_distribution = distribution.copy()
        for subtotal, count in distribution.items():
            next_distribution[subtotal + rank] = next_distribution.get(subtotal + rank, 0) + count
        distribution = next_distribution
    return sum(count for subtotal, count in distribution.items()
               if min(subtotal, total - subtotal) <= distance) / (2 ** len(differences))


def cliffs_delta(left, right):
    return sum((a > b) - (a < b) for a in left for b in right) / (len(left) * len(right))


def metrics(measured, mode):
    seconds = measured["elapsed_s"]
    shipments = measured["committed_shipments"]
    if seconds <= 0 or shipments <= 0:
        raise ValueError("completed E3 run has no elapsed time or shipments")
    audit = measured.get("audit_samples", [])
    calldata = measured.get("calldata_samples", [])
    gas = measured.get("gas_samples", [])
    return {
        "throughput_lots_min": shipments * 60 / seconds,
        "throughput_events_s": measured["committed_events"] / seconds,
        "audit_p50_ms": percentile(audit, 0.5),
        "audit_p95_ms": percentile(audit, 0.95),
        "onchain_bytes_per_shipment": None if mode == "fabric-only" else sum(calldata) / shipments,
        "gas_per_shipment": None if mode == "fabric-only" else sum(gas) / shipments,
    }


def analyze(records, allow_partial=False):
    by_mode = defaultdict(dict)
    statuses = defaultdict(int)
    commits = set()
    gates = set()
    provers = set()
    for record in records:
        mode, seed = record["mode"], record["seed"]
        if mode not in MODES or not isinstance(seed, int) or not 0 <= seed < 30:
            raise ValueError("unknown mode or seed outside 0..29")
        if seed in by_mode[mode]:
            raise ValueError(f"duplicate E3 {mode} seed {seed}")
        if (record["np"], record["lambda_events_per_min"], record["duration_min"],
                record["shipment"], record["epochs_target"]) != (125, 480, 60, 64, 60):
            raise ValueError("E3 workload does not match the fixed formal design")
        by_mode[mode][seed] = record
        measured = record["measurement"]
        statuses[measured["status"]] += 1
        commits.add(record["git_commit"])
        gates.add(record["gate_transaction_hash"])
        provers.add(record["prover"])
        if measured["status"] == "ok":
            if (measured.get("epochs_completed") != 60 or not measured.get("queue_drained")
                    or measured.get("committed_events") != measured.get("offered_events")
                    or measured.get("committed_shipments", 0) <= 0
                    or measured.get("committed_shipments") != measured.get("offered_shipments")):
                raise ValueError(f"E3 {mode} seed {seed} claims success without 60 drained epochs")
            shipments = measured["committed_shipments"]
            if (not 8 <= measured["committed_events"] / shipments <= 64
                    or len(measured.get("audit_samples", [])) < shipments):
                raise ValueError(f"E3 {mode} seed {seed} has inconsistent shipment/audit counts")
            transactions = measured.get("transactions", [])
            if mode == "fabric-only" and transactions:
                raise ValueError("Fabric-only E3 has unexpected L1 transactions")
            if mode == "proposed" and len(transactions) != 60:
                raise ValueError("proposed E3 needs one Anvil receipt per epoch")
            if mode == "hash-on-chain" and len(transactions) != measured["committed_shipments"]:
                raise ValueError("hash E3 needs one Anvil receipt per shipment")
            if mode == "vecro-adapted" and len(transactions) < measured["committed_shipments"]:
                raise ValueError("adapted token E3 needs at least one mint per shipment")
            if any(not tx.get("transactionHash") or not tx.get("gasUsed") for tx in transactions):
                raise ValueError("E3 Anvil receipt is missing transaction hash or gas")
    if len(commits) != 1:
        raise ValueError("E3 records have different Git commits")
    if len(gates) != 1 or len(provers) != 1:
        raise ValueError("E3 records used different gate receipts or provers")
    for seed in range(30):
        offered = {by_mode[mode][seed]["measurement"]["offered_events"]
                   for mode in MODES if seed in by_mode[mode]}
        if len(offered) > 1:
            raise ValueError(f"E3 seed {seed} did not use the same offered workload across modes")
    full = all(set(by_mode[mode]) == set(range(30)) for mode in MODES)
    successful = full and statuses == {"ok": 120}
    if not allow_partial and not successful:
        raise ValueError(f"formal E3 needs all 4 modes x 30 successful paired seeds; statuses={dict(statuses)}")
    summary = {"formal": successful, "runs": len(records), "statuses": dict(statuses),
               "git_commit": next(iter(commits)), "modes": {}, "paired_tests": []}
    for mode in MODES:
        valid = [metrics(by_mode[mode][seed]["measurement"], mode) for seed in sorted(by_mode[mode])
                 if by_mode[mode][seed]["measurement"]["status"] == "ok"]
        summary["modes"][mode] = {"n_ok": len(valid), "metrics": {}}
        for name in ("throughput_lots_min", "throughput_events_s", "audit_p50_ms",
                     "audit_p95_ms", "onchain_bytes_per_shipment", "gas_per_shipment"):
            values = [row[name] for row in valid if row[name] is not None]
            summary["modes"][mode]["metrics"][name] = None if not values else {
                "mean": statistics.fmean(values), "median": percentile(values, 0.5),
                "iqr": [percentile(values, 0.25), percentile(values, 0.75)],
                "p95": percentile(values, 0.95), "bootstrap_mean_ci95": bootstrap_mean_ci(values),
            }
    if successful:
        for left_mode, right_mode in itertools.combinations(MODES, 2):
            left = [metrics(by_mode[left_mode][seed]["measurement"], left_mode)["throughput_lots_min"]
                    for seed in range(30)]
            right = [metrics(by_mode[right_mode][seed]["measurement"], right_mode)["throughput_lots_min"]
                     for seed in range(30)]
            p = wilcoxon_exact(left, right)
            summary["paired_tests"].append({"metric": "throughput_lots_min", "left": left_mode,
                "right": right_mode, "wilcoxon_exact_p": p, "bonferroni_p": min(1.0, p * 6),
                "significant_0_05": p * 6 < 0.05, "cliffs_delta": cliffs_delta(left, right)})
    return summary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--allow-partial", action="store_true", help="pilot only; no formal p-values")
    args = parser.parse_args()
    records = [json.loads(path.read_text()) for path in sorted(args.raw_dir.glob("*.json"))]
    if not records:
        parser.error("no E3 raw records found")
    summary = analyze(records, args.allow_partial)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"E3 formal={summary['formal']} runs={summary['runs']} -> {args.out}")


if __name__ == "__main__":
    main()
