#!/usr/bin/env python3
"""E4 sweep orchestrator harness.

Reads the committed plan (scripts/e4_plan.py output) and executes each run,
writing one cell-result JSONL line per run plus a committed run manifest.

Two executors:
  * mock   -- deterministic synthetic metrics; harness/schema/analysis self-test only.
              NOT valid E4 evidence.
  * real-e3 -- STUB. Plugs into the E3 aggregate proof pipeline
              (Fabric -> leaf proofs -> SP1 aggregate -> Anvil receipt) once the E3
              gate passes. See RealE3Executor below (handoff with Hữu Trí).

Measurement rules enforced here (issue #22):
  * Each run targets exactly 4 complete epochs.
  * When injection stops, the queue must drain before finalizing.
  * If wall-time is hit, the run is marked 'saturated' (never dropped/replaced).
  * Other infra cuts -> 'censored'.

Stdlib only. Timestamps are passed in via --created-at (scripts avoid wall-clock
so runs are reproducible / resumable).
"""

import argparse
import csv
import json
import math
import random
import statistics
from pathlib import Path

LAMBDA_EVENTS_PER_MIN = 3.84
EPOCHS_TARGET = 4


# ----------------------------- workload -----------------------------------

def poisson_event_count(rng, expected):
    """Knuth's algorithm for a Poisson sample (stdlib only)."""
    if expected <= 0:
        return 0
    l = math.exp(-expected)
    k = 0
    p = 1.0
    while True:
        k += 1
        p *= rng.random()
        if p <= l:
            return k - 1


def generate_offered_events(seed, np_, epoch_s, epochs):
    """Total events offered by `np_` producers over `epochs` epochs of `epoch_s` seconds.

    Each producer is Poisson with mean LAMBDA_EVENTS_PER_MIN per minute. Deterministic
    per (seed, np_, epoch_s). Returns total offered event count.
    """
    rng = random.Random(seed ^ 0x00E4_0000)
    per_epoch_expected = LAMBDA_EVENTS_PER_MIN * (epoch_s / 60.0)  # per producer per epoch
    total = 0
    for _epoch in range(epochs):
        # Sum of Np independent Poisson(per_epoch_expected) == Poisson(Np * per_epoch_expected).
        total += poisson_event_count(rng, np_ * per_epoch_expected)
    return total


# ----------------------------- stats helper --------------------------------

def stat(values):
    values = [float(v) for v in values]
    if not values:
        return {"count": 0, "mean": 0.0, "p50": 0.0, "p95": 0.0,
                "p99": None, "min": None, "max": None, "sum": 0.0}
    s = sorted(values)

    def pct(p):
        if len(s) == 1:
            return s[0]
        rank = p / 100 * (len(s) - 1)
        lo = int(rank)
        hi = min(lo + 1, len(s) - 1)
        return s[lo] + (s[hi] - s[lo]) * (rank - lo)

    return {
        "count": len(s),
        "mean": statistics.fmean(s),
        "p50": pct(50),
        "p95": pct(95),
        "p99": pct(99),
        "min": s[0],
        "max": s[-1],
        "sum": sum(s),
    }


# ----------------------------- executors -----------------------------------

class MockExecutor:
    """Deterministic synthetic executor for harness/schema/analysis self-test.

    Models a bounded service capacity so high-Np cells saturate — this exercises the
    analyzer's censored/saturated handling. Output is NOT valid E4 evidence.
    """

    name = "mock"

    def __init__(self, wall_time_s=None):
        self.wall_time_s = wall_time_s

    def run(self, row):
        np_ = int(row["np"])
        shipment = int(row["shipment"])
        epoch_s = int(row["epoch_s"])
        seed = int(row["seed"])
        rng = random.Random(seed ^ 0x00E4_9999)

        offered = generate_offered_events(seed, np_, epoch_s, EPOCHS_TARGET)

        # Synthetic per-lot proving cost grows with shipment size; service rate is
        # bounded so throughput plateaus and large Np saturates.
        leaf_ms_mean = 40.0 + 6.0 * shipment
        lots_offered = max(1, offered // shipment)
        aggregate_ms_mean = 800.0 + 55.0 * math.log2(max(2, lots_offered))

        # Service capacity (events/s) the pipeline can drain in steady state.
        service_eps = 900.0 / (1.0 + shipment / 32.0)
        offered_eps = offered / (EPOCHS_TARGET * epoch_s)

        # Wall-time budget: allow drain slack beyond the 4 epochs.
        wall = self.wall_time_s if self.wall_time_s is not None else EPOCHS_TARGET * epoch_s * 3.0

        if offered_eps <= service_eps:
            committed = offered
            drain_extra_s = (offered_eps / service_eps) * epoch_s * 0.25
            elapsed = EPOCHS_TARGET * epoch_s + drain_extra_s
            status = "ok"
            drained = True
            censor_reason = None
            epochs_completed = EPOCHS_TARGET
        else:
            # Queue grows unboundedly; only service_eps * wall can ever commit.
            committed = min(offered, int(service_eps * wall))
            elapsed = wall
            status = "saturated"
            drained = False
            censor_reason = "wall_time"
            # Steady-state throughput reached but 4 clean epochs never completed.
            epochs_completed = min(EPOCHS_TARGET, int((service_eps * wall) / max(1, offered / EPOCHS_TARGET)))

        committed_lots = max(1, committed // shipment)
        throughput = committed / elapsed if elapsed else 0.0
        completion = committed / offered if offered else 0.0

        leaf_samples = [max(1.0, rng.gauss(leaf_ms_mean, leaf_ms_mean * 0.08))
                        for _ in range(min(committed_lots, 200))]
        agg_samples = [max(1.0, rng.gauss(aggregate_ms_mean, aggregate_ms_mean * 0.05))
                       for _ in range(min(EPOCHS_TARGET, 4))]
        audit_mean = 1500.0 + leaf_ms_mean + aggregate_ms_mean
        audit_samples = [max(1.0, rng.gauss(audit_mean, audit_mean * 0.1))
                         for _ in range(min(committed_lots, 200))]
        gas_samples = [rng.gauss(240000 + 320 * shipment, 5000) for _ in range(min(EPOCHS_TARGET, 4))]
        calldata_samples = [float(320 + 64 * math.ceil(math.log2(max(2, committed_lots))))
                            for _ in range(min(EPOCHS_TARGET, 4))]

        return {
            "status": status,
            "censor_reason": censor_reason,
            "epochs_completed": epochs_completed,
            "queue_drained": drained,
            "wall_time_s": wall,
            "elapsed_s": round(elapsed, 3),
            "offered_events": offered,
            "committed_events": committed,
            "throughput_eps": round(throughput, 4),
            "completion_rate": round(completion, 6),
            "leaf_proving_ms": stat(leaf_samples),
            "aggregate_proving_ms": stat(agg_samples),
            "audit_latency_ms": stat(audit_samples),
            "gas_used": stat(gas_samples),
            "calldata_bytes": stat(calldata_samples),
            "split_host": False,
            "split_host_rtt_ms": None,
            "e3_manifest_ref": None,
            "host": "mock",
        }


class RealE3Executor:
    """Official E4 executor. BLOCKED until the E3 aggregate proof gate passes.

    Integration seam (handoff with Hữu Trí — do NOT redefine proof/root format here):
      1. Generate the Poisson workload for (seed, np, epoch_s, 4 epochs).
      2. Ingest events into Fabric (2 org / 2 peer / 3 Raft orderer) via Gateway.
      3. Form lots of `shipment` events; produce leaf proofs.
      4. Produce the SP1 aggregate proof using E3's public-values / ABI / manifest.
      5. Submit to Anvil; record real receipt, gas, calldata bytes.
      6. On stop: drain the queue; on wall-time: mark 'saturated'.
    """

    name = "real-e3"

    def __init__(self, wall_time_s=None, split_host=False, e3_manifest_ref=None):
        self.wall_time_s = wall_time_s
        self.split_host = split_host
        self.e3_manifest_ref = e3_manifest_ref

    def run(self, row):
        raise NotImplementedError(
            "RealE3Executor is blocked until the E3 aggregate proof gate passes "
            "(Fabric -> leaf proofs -> SP1 aggregate -> Anvil receipt). "
            "Wire this to the E3 pipeline using E3's manifest/public-values/ABI."
        )


EXECUTORS = {"mock": MockExecutor, "real-e3": RealE3Executor}


# ----------------------------- main ----------------------------------------

def read_plan(path, phase):
    with Path(path).open(newline="") as f:
        rows = list(csv.DictReader(f))
    if phase != "all":
        rows = [r for r in rows if r["phase"] == phase]
    if not rows:
        raise SystemExit(f"No plan rows for phase={phase} in {path}")
    return rows


def build_manifest(phase, rows, executor, args):
    nps = sorted({int(r["np"]) for r in rows})
    ships = sorted({int(r["shipment"]) for r in rows})
    epochs = sorted({int(r["epoch_s"]) for r in rows})
    cells = len({r["cell_id"] for r in rows})
    seeds_per_cell = int(rows[0]["seeds_per_cell"])
    return {
        "experiment": "E4",
        "phase": phase if phase != "all" else "screening",
        "created_at": args.created_at,
        "command": args.command or "",
        "executor": executor.name,
        "grid": {"np": nps, "shipment": ships, "epoch_s": epochs},
        "epochs_target": EPOCHS_TARGET,
        "lambda_events_per_min": LAMBDA_EVENTS_PER_MIN,
        "seeds_per_cell": seeds_per_cell,
        "cells_total": cells,
        "runs_total": len(rows),
        "wall_time_s": args.wall_time_s,
        "raw_path": str(args.out),
        "plan_path": str(args.plan),
        "split_host": getattr(executor, "split_host", False),
        "split_host_rtt_ms": None,
        "topology": {
            "fabric_orgs": 2,
            "fabric_peers_per_org": 2,
            "raft_orderers": 3,
            "gateway": "fabric-gateway",
            "l1": "anvil",
            "prover": "mock" if executor.name == "mock" else args.prover,
        },
        "preflight_ab": None,
        "e3_gate_passed": executor.name == "real-e3",
        "e3_manifest_ref": getattr(executor, "e3_manifest_ref", None),
        "software": {
            "git_commit": args.git_commit,
            "sp1_version": None,
            "fabric_version": None,
            "anvil_version": None,
        },
    }


def main():
    parser = argparse.ArgumentParser(description="E4 sweep orchestrator")
    parser.add_argument("--plan", type=Path, required=True, help="Plan CSV from e4_plan.py")
    parser.add_argument("--phase", choices=["screening", "confirmation", "all"], default="all")
    parser.add_argument("--executor", choices=list(EXECUTORS), default="mock")
    parser.add_argument("--out", type=Path, required=True, help="Raw JSONL output (not committed)")
    parser.add_argument("--manifest-out", type=Path, required=True, help="Run manifest JSON (committed)")
    parser.add_argument("--wall-time-s", type=float, default=None)
    parser.add_argument("--prover", default="cpu")
    parser.add_argument("--created-at", default="1970-01-01T00:00:00Z",
                        help="ISO-8601 run timestamp (pass real time from the shell).")
    parser.add_argument("--command", default=None)
    parser.add_argument("--git-commit", default=None)
    parser.add_argument("--limit", type=int, default=None, help="Only run first N rows (smoke).")
    args = parser.parse_args()

    rows = read_plan(args.plan, args.phase)
    if args.limit:
        rows = rows[: args.limit]

    executor = EXECUTORS[args.executor](wall_time_s=args.wall_time_s)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    n_ok = n_sat = n_cen = 0
    with args.out.open("w") as f:
        for i, row in enumerate(rows, 1):
            result = executor.run(row)
            record = {
                "kind": "cell",
                "run_id": row["run_id"],
                "phase": row["phase"],
                "cell_id": row["cell_id"],
                "np": int(row["np"]),
                "shipment": int(row["shipment"]),
                "epoch_s": int(row["epoch_s"]),
                "seed": int(row["seed"]),
                "lambda_events_per_min": LAMBDA_EVENTS_PER_MIN,
                "offered_load_epm": round(LAMBDA_EVENTS_PER_MIN * int(row["np"]), 4),
                "epochs_target": EPOCHS_TARGET,
                "executor": executor.name,
                **result,
            }
            f.write(json.dumps(record) + "\n")
            status = record["status"]
            n_ok += status == "ok"
            n_sat += status == "saturated"
            n_cen += status == "censored"
            if i % 25 == 0 or i == len(rows):
                print(f"  ran {i}/{len(rows)} (ok={n_ok} saturated={n_sat} censored={n_cen})")

    manifest = build_manifest(args.phase, rows, executor, args)
    args.manifest_out.parent.mkdir(parents=True, exist_ok=True)
    args.manifest_out.write_text(json.dumps(manifest, indent=2) + "\n")

    print(f"Wrote raw JSONL: {args.out} ({len(rows)} runs; ok={n_ok} saturated={n_sat} censored={n_cen})")
    print(f"Wrote manifest:  {args.manifest_out}")
    if executor.name == "mock":
        print("NOTE: mock executor output is a harness self-test, NOT valid E4 evidence.")


if __name__ == "__main__":
    main()
