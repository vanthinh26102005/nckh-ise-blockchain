#!/usr/bin/env python3
"""Run one real, paired E3 baseline cell; raw evidence stays outside Git."""
import argparse
import os
import json
import subprocess
import tomllib
from pathlib import Path
from e4_preflight import hardware_metadata, mode_order, policy_manifest, validate_receipt, validate_receipt_chain, verify_topology


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
    parser.add_argument("--anvil-rpc", default="http://127.0.0.1:8545")
    parser.add_argument("--git-commit", required=True)
    parser.add_argument("--policy-manifest", type=Path, required=True)
    parser.add_argument("--isolation-manifest",type=Path,required=True)
    args = parser.parse_args()
    if not 0 <= args.seed < 30 or args.wall_time_s <= 3600:
        parser.error("formal E3 requires seed 0..29 and wall-time > 3600 s")
    if not args.binary.is_file() or args.out.resolve().is_relative_to(REPO) or args.out.exists():
        parser.error("binary must exist and raw output must be a new file outside Git")
    isolation=json.loads(args.isolation_manifest.read_text())
    if (isolation.get("protocolVersion")!=2 or isolation.get("sourceRevision")!=args.git_commit
            or isolation.get("freshLedger") is not True or isolation.get("seed")!=args.seed or isolation.get("mode")!=args.mode
            or isolation.get("computeHost")!=__import__('socket').gethostname()
            or isolation.get("order")!=mode_order(args.seed).index(args.mode)
            or not isolation.get("namespace") or not Path(isolation.get("runtimePath","")).is_absolute()):
        parser.error("fresh allocation-owned Fabric isolation record is required for this paired cell")
    registry_sha, accepted_policies = policy_manifest(args.policy_manifest)
    configured=json.loads(args.policy_manifest.read_bytes())
    axes={"seed":args.seed,"np":125,"shipment":64,"epoch_s":60,"epochs":60}
    selected=[workload for workload in configured.get("workloads",[]) if all(workload.get(key)==value for key,value in axes.items())]
    if len(selected)!=1:
        parser.error("trusted manifest must contain exactly one prebuilt workload for the E3 seed/axes")
    trusted_workload=selected[0]
    gate = json.loads(args.gate.read_text())
    if gate.get("protocolVersion") != 2 or gate.get("verificationKind") != "sp1-groth16" or not gate.get("registry"):
        parser.error("E3 gate must contain policy-v2 real SP1 Groth16 and registry evidence")
    if (gate.get("status") != "passed" or gate.get("anvilReceiptStatus") != 1
            or not gate.get("transactionHash")):
        parser.error("E3 aggregate gate must pass before formal comparison")
    if gate.get("gitCommit") != args.git_commit:
        parser.error("E3 gate and comparison must use the same Git commit")
    topology = verify_topology(args.fabric_gateway, args.anvil_rpc)
    packages = tomllib.loads((REPO / "crates/sp1-e2e/Cargo.lock").read_text())["package"]
    sp1_version = next(package["version"] for package in packages if package["name"] == "sp1-sdk")
    command = [str(args.binary.resolve()), "--mode", args.mode, "--seed", str(args.seed),
               "--np", "125", "--shipment", "64", "--epoch-s", "60", "--epochs", "60",
               "--wall-time-s", str(args.wall_time_s), "--prover", args.prover,
               "--fabric-gateway", args.fabric_gateway, "--anchor-server", args.anchor_server]
    try:
        run = subprocess.run(command, env=dict(os.environ, POLICY_MANIFEST=str(args.policy_manifest.resolve())), text=True, capture_output=True,
                             timeout=args.wall_time_s + 900)
        if run.returncode:
            raise RuntimeError(f"E3 binary exited {run.returncode}: {run.stderr[-500:]}")
        measured = json.loads(run.stdout.strip().splitlines()[-1])
        if measured.get("mode") != args.mode:
            raise RuntimeError("E3 binary returned a different baseline mode")
        if measured.get("status")=="ok" and measured.get("sourceRevision")!=args.git_commit:
            raise RuntimeError("E3 binary sourceRevision differs from --git-commit")
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
    if measured.get("status") == "ok":
        if measured.get("protocolVersion") != 2 or not measured.get("workload_digest"):
            raise RuntimeError("E3 measured result requires policy-v2 and actual workload digest")
        if measured.get("sourceRevision")!=args.git_commit or measured.get("registryManifestSHA256")!=registry_sha:
            raise RuntimeError("E3 measured source or registry manifest differs from configured provenance")
        for receipt in measured.get("transactions", []):
            validate_receipt(receipt, proof=args.mode in ("vecro-adapted", "proposed"))
        if args.mode in ("vecro-adapted", "proposed"):
            validate_receipt_chain(measured.get("transactions",[]))
        if (not isinstance(measured.get("elapsed_s"),(int,float)) or not __import__('math').isfinite(measured["elapsed_s"]) or measured["elapsed_s"]<3600
                or measured.get("offered_events")!=trusted_workload.get("offeredEvents")
                or measured.get("offered_shipments")!=trusted_workload.get("offeredShipments")
                or str(measured.get("workload_digest","")).removeprefix("0x").lower()!=str(trusted_workload.get("digest","")).removeprefix("0x").lower()):
            raise RuntimeError("E3 result differs from the paced trusted prebuilt workload")
    record = {"experiment": "E3", "protocolVersion": 2,
              "registry_manifest_sha256": registry_sha, "ledgerIsolation":isolation, "acceptedPolicies": accepted_policies,
              "trustedWorkload":trusted_workload,
              "hardware": hardware_metadata(),
              "software": {"sp1_version": sp1_version, "fabric_images": topology["fabricContainers"], "anvil_version": topology["anvilClientVersion"]},
              "mode_order": mode_order(args.seed),
              "mode_order_index": mode_order(args.seed).index(args.mode), "git_commit": args.git_commit,
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
