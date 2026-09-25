#!/usr/bin/env python3
"""Run one real, paired E3 baseline cell; raw evidence stays outside Git."""
import argparse
import json
import subprocess
from pathlib import Path


MODES = ("fabric-only", "hash-on-chain", "vecro-adapted", "proposed")
REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--gate", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--mode", choices=MODES, required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--prover", choices=("cpu", "cuda"), default="cuda")
    parser.add_argument("--wall-time-s", type=int, default=7200)
    parser.add_argument("--fabric-gateway", default="http://127.0.0.1:8080")
    parser.add_argument("--anchor-server", default="http://127.0.0.1:8546")
    parser.add_argument("--git-commit", required=True)
    args = parser.parse_args()
    if not 0 <= args.seed < 30 or args.wall_time_s <= 3600:
        parser.error("formal E3 requires seed 0..29 and wall-time > 3600 s")
    if not args.binary.is_file() or args.out.resolve().is_relative_to(REPO) or args.out.exists():
        parser.error("binary must exist and raw output must be a new file outside Git")
    gate = json.loads(args.gate.read_text())
    if (gate.get("status") != "passed" or gate.get("anvilReceiptStatus") != 1
            or not gate.get("transactionHash")):
        parser.error("E3 aggregate gate must pass before formal comparison")
    if gate.get("gitCommit") != args.git_commit:
        parser.error("E3 gate and comparison must use the same Git commit")
    command = [str(args.binary.resolve()), "--mode", args.mode, "--seed", str(args.seed),
               "--np", "125", "--shipment", "64", "--epoch-s", "60", "--epochs", "60",
               "--wall-time-s", str(args.wall_time_s), "--prover", args.prover,
               "--fabric-gateway", args.fabric_gateway, "--anchor-server", args.anchor_server]
    try:
        run = subprocess.run(command, text=True, capture_output=True,
                             timeout=args.wall_time_s + 900)
        if run.returncode:
            raise RuntimeError(f"E3 binary exited {run.returncode}: {run.stderr[-500:]}")
        measured = json.loads(run.stdout.strip().splitlines()[-1])
        if measured.get("mode") != args.mode:
            raise RuntimeError("E3 binary returned a different baseline mode")
    except (OSError, RuntimeError, ValueError, IndexError, subprocess.TimeoutExpired) as error:
        measured = {"mode": args.mode,
                    "status": "saturated" if isinstance(error, subprocess.TimeoutExpired) else "censored",
                    "censor_reason": str(error)[:700],
                    "epochs_completed": 0, "queue_drained": False, "elapsed_s": None,
                    "offered_events": None, "committed_events": None,
                    "offered_shipments": None, "committed_shipments": None,
                    "leaf_samples": [], "aggregate_samples": [], "audit_samples": [],
                    "gas_samples": [], "calldata_samples": [], "transactions": []}
    if measured.get("status") == "ok" and (
            measured.get("epochs_completed") != 60 or not measured.get("queue_drained")
            or measured.get("committed_events") != measured.get("offered_events")
            or measured.get("committed_shipments") != measured.get("offered_shipments")):
        raise RuntimeError("E3 result claims success without 60 drained epochs")
    if args.mode != "fabric-only" and measured.get("status") == "ok" and not measured.get("transactions"):
        raise RuntimeError("E3 L1 baseline claims success without Anvil receipts")
    record = {"experiment": "E3", "git_commit": args.git_commit,
              "gate_transaction_hash": gate["transactionHash"], "mode": args.mode,
              "seed": args.seed, "prover": args.prover, "np": 125,
              "lambda_events_per_min": 480, "duration_min": 60,
              "shipment": 64, "epoch_s": 60, "epochs_target": 60,
              "measurement": measured}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        output.write(json.dumps(record, indent=2) + "\n")
    print(f"E3 {args.mode} seed {args.seed}: {measured['status']} -> {args.out}")


if __name__ == "__main__":
    main()
