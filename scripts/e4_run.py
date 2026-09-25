#!/usr/bin/env python3
"""E4 sweep orchestrator harness.

Reads the committed plan (scripts/e4_plan.py output) and executes each run,
writing one cell-result JSONL line per run plus a committed run manifest.

Two executors:
  * mock   -- deterministic synthetic metrics; harness/schema/analysis self-test only.
              NOT valid E4 evidence.
  * real-e3 -- invokes the Rust E3 cell binary (Fabric -> compressed leaves ->
               recursive SP1 aggregate -> Anvil receipt).

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
import socket
import subprocess
import statistics
from pathlib import Path
from urllib.parse import urlparse

LAMBDA_EVENTS_PER_MIN = 3.84
EPOCHS_TARGET = 4


# ----------------------------- workload -----------------------------------

def poisson_event_count(rng, expected):
    """Exact Poisson sample, chunked so exp(-lambda) never underflows."""
    if expected <= 0:
        return 0
    total = 0
    while expected > 0:
        chunk = min(expected, 64.0)
        threshold = math.exp(-chunk)
        product = 1.0
        count = 0
        while product > threshold:
            product *= rng.random()
            count += 1
        total += count - 1
        expected -= chunk
    return total


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
        return {"count": 0, "mean": None, "p50": None, "p95": None,
                "p99": None, "min": None, "max": None, "sum": None}
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
    """Official E4 executor; the Rust binary owns the proof and receipt format."""

    name = "real-e3"

    def __init__(self, wall_time_s=None, split_host=False, e3_manifest_ref=None,
                 binary=None, fabric_gateway=None, anchor_server=None, prover="cpu"):
        self.wall_time_s = wall_time_s
        self.split_host = split_host
        self.e3_manifest_ref = e3_manifest_ref
        self.binary = binary
        self.fabric_gateway = fabric_gateway
        self.anchor_server = anchor_server
        self.prover = prover

    def run(self, row):
        wall = self.wall_time_s
        command = [str(self.binary), "--np", str(row["np"]), "--shipment", str(row["shipment"]),
                   "--epoch-s", str(row["epoch_s"]), "--seed", str(row["seed"]),
                   "--wall-time-s", str(math.ceil(wall)), "--prover", self.prover,
                   "--fabric-gateway", self.fabric_gateway, "--anchor-server", self.anchor_server]
        try:
            result = subprocess.run(command, text=True, capture_output=True, check=True,
                                    timeout=wall + 600)
            measured = json.loads(result.stdout.strip().splitlines()[-1])
        except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired, ValueError, IndexError) as exc:
            measured = {"status": "saturated" if isinstance(exc, subprocess.TimeoutExpired) else "censored",
                        "censor_reason": str(exc),
                        "epochs_completed": 0, "queue_drained": False, "elapsed_s": None,
                        "offered_events": None, "committed_events": None}
        offered = measured["offered_events"]
        committed = measured["committed_events"]
        elapsed = measured["elapsed_s"]
        return {
            "status": measured["status"], "censor_reason": measured.get("censor_reason"),
            "epochs_completed": measured["epochs_completed"],
            "queue_drained": measured["queue_drained"],
            "wall_time_s": wall, "elapsed_s": elapsed,
            "offered_events": offered, "committed_events": committed,
            "offered_shipments": measured.get("offered_shipments"),
            "committed_shipments": measured.get("committed_shipments"),
            "throughput_eps": committed / elapsed if committed is not None and elapsed else None,
            "completion_rate": committed / offered if committed is not None and offered else None,
            "leaf_proving_ms": stat(measured.get("leaf_samples", [])),
            "aggregate_proving_ms": stat(measured.get("aggregate_samples", [])),
            "audit_latency_ms": stat(measured.get("audit_samples", [])),
            "gas_used": stat(measured.get("gas_samples", [])),
            "calldata_bytes": stat(measured.get("calldata_samples", [])),
            "transactions": measured.get("transactions", []),
            "split_host": self.split_host, "split_host_rtt_ms": None,
            "e3_manifest_ref": str(self.e3_manifest_ref),
            "host": measured.get("host"), "rng": measured.get("rng"),
        }


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
        "phase": phase,
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
            "fabric_peers_per_org": 1,
            "raft_orderers": 3,
            "gateway": "fabric-gateway",
            "l1": "anvil",
            "prover": "mock" if executor.name == "mock" else args.prover,
        },
        "preflight_ab": args.preflight,
        "e3_gate_passed": args.gate is not None,
        "e3_manifest_ref": getattr(executor, "e3_manifest_ref", None),
        "software": {
            "git_commit": args.git_commit,
            "sp1_version": args.preflight.get("sp1SdkVersion") if args.preflight else None,
            "fabric_images": args.preflight.get("topologyEvidence", {}).get("fabricContainers") if args.preflight else None,
            "anvil_version": args.preflight.get("topologyEvidence", {}).get("anvilClientVersion") if args.preflight else None,
        },
    }


def main():
    parser = argparse.ArgumentParser(description="E4 sweep orchestrator")
    parser.add_argument("--plan", type=Path, required=True, help="Plan CSV from e4_plan.py")
    parser.add_argument("--phase", choices=["screening", "confirmation"], required=True)
    parser.add_argument("--executor", choices=list(EXECUTORS), default="mock")
    parser.add_argument("--out", type=Path, required=True, help="Raw JSONL output (not committed)")
    parser.add_argument("--manifest-out", type=Path, required=True, help="Run manifest JSON (committed)")
    parser.add_argument("--wall-time-s", type=float, default=None)
    parser.add_argument("--prover", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--real-binary", type=Path)
    parser.add_argument("--fabric-gateway", default="http://127.0.0.1:8080")
    parser.add_argument("--anchor-server", default="http://127.0.0.1:8546")
    parser.add_argument("--e3-gate-manifest", type=Path)
    parser.add_argument("--preflight-ab", type=Path)
    parser.add_argument("--created-at", default="1970-01-01T00:00:00Z",
                        help="ISO-8601 run timestamp (pass real time from the shell).")
    parser.add_argument("--command", default=None)
    parser.add_argument("--git-commit", default=None)
    parser.add_argument("--limit", type=int, default=None, help="Only run first N rows (smoke).")
    parser.add_argument("--run-id", help="Run exactly one planned row (Slurm array shard).")
    parser.add_argument("--plan-index", type=int, help="Zero-based row index within the selected phase (Slurm array).")
    args = parser.parse_args()

    rows = read_plan(args.plan, args.phase)
    if args.run_id and args.plan_index is not None:
        parser.error("--run-id and --plan-index cannot be combined")
    if args.plan_index is not None:
        if not 0 <= args.plan_index < len(rows):
            parser.error("--plan-index is outside the selected phase")
        rows = [rows[args.plan_index]]
    if args.run_id:
        rows = [row for row in rows if row["run_id"] == args.run_id]
        if len(rows) != 1:
            parser.error("--run-id must identify exactly one row in the selected phase")
    if (args.run_id or args.plan_index is not None) and args.limit is not None:
        parser.error("--run-id/--plan-index and --limit cannot be combined")
    if args.limit is not None:
        if args.limit < 1:
            parser.error("--limit must be positive")
        rows = rows[: args.limit]

    args.gate = None
    args.preflight = None
    if args.executor == "real-e3":
        if not args.git_commit or len(args.git_commit) < 7:
            parser.error("--git-commit is required for real-e3")
        if not args.real_binary or not args.real_binary.is_file():
            parser.error("--real-binary must point to a built e4_real_cell binary")
        if args.wall_time_s is None or args.wall_time_s <= 0:
            parser.error("--wall-time-s must be positive for real-e3")
        if args.out.resolve().is_relative_to(Path(__file__).resolve().parents[1]):
            parser.error("real E4 raw JSONL must be outside the Git repository")
        if args.out.exists() or args.manifest_out.exists():
            parser.error("real E4 output already exists; use a new path to preserve earlier data")
        if not args.e3_gate_manifest or not args.e3_gate_manifest.is_file():
            parser.error("--e3-gate-manifest is required for real-e3")
        args.gate = json.loads(args.e3_gate_manifest.read_text())
        if (args.gate.get("status") != "passed" or args.gate.get("anvilReceiptStatus") != 1
                or not args.gate.get("transactionHash")):
            parser.error("E3 gate manifest must contain a passed Anvil receipt")
        if args.gate.get("gitCommit") != args.git_commit:
            parser.error("E3 gate and E4 run must use the same Git commit")
        if not args.preflight_ab or not args.preflight_ab.is_file():
            parser.error("--preflight-ab is required for real-e3")
        args.preflight = json.loads(args.preflight_ab.read_text())
        metrics = ("leaf8_cpu_ms", "leaf8_gpu_ms", "leaf64_cpu_ms", "leaf64_gpu_ms",
                   "aggregate2_cpu_ms", "aggregate2_gpu_ms")
        if any(type(args.preflight.get(metric)) not in (int, float)
               or not math.isfinite(args.preflight[metric])
               or args.preflight[metric] <= 0 for metric in metrics):
            parser.error("preflight A/B must have six measured CPU/GPU timing fields")
        if args.preflight.get("gitCommit") != args.git_commit:
            parser.error("E4 preflight and run must use the same Git commit")
        if args.preflight.get("computeHost") != socket.gethostname():
            parser.error("E4 run must use the same compute host as CPU/GPU preflight")
        required_containers = {
            "e2-orderer1.example.com", "e2-orderer2.example.com", "e2-orderer3.example.com",
            "e2-peer0.org1.example.com", "e2-peer0.org2.example.com",
        }
        evidence = args.preflight.get("topologyEvidence", {})
        if set(evidence.get("fabricContainers", {})) != required_containers or not evidence.get("anvilChainId"):
            parser.error("E4 preflight is missing observed Fabric/Anvil topology evidence")
        local = {"localhost", "127.0.0.1", "::1"}
        split_host = (urlparse(args.fabric_gateway).hostname not in local
                      or urlparse(args.anchor_server).hostname not in local)
        executor = RealE3Executor(args.wall_time_s, split_host=split_host,
                                  e3_manifest_ref=args.e3_gate_manifest,
                                  binary=args.real_binary, fabric_gateway=args.fabric_gateway,
                                  anchor_server=args.anchor_server, prover=args.prover)
    else:
        executor = MockExecutor(args.wall_time_s)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    n_ok = n_sat = n_cen = 0
    with args.out.open("x" if args.executor == "real-e3" else "w") as f:
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
    with args.manifest_out.open("x" if args.executor == "real-e3" else "w") as output:
        output.write(json.dumps(manifest, indent=2) + "\n")

    print(f"Wrote raw JSONL: {args.out} ({len(rows)} runs; ok={n_ok} saturated={n_sat} censored={n_cen})")
    print(f"Wrote manifest:  {args.manifest_out}")
    if executor.name == "mock":
        print("NOTE: mock executor output is a harness self-test, NOT valid E4 evidence.")


if __name__ == "__main__":
    main()
