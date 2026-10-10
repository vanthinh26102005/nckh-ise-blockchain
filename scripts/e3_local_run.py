#!/usr/bin/env python3
"""Run one bounded CPU E3 mode on a fresh local Fabric/Anvil stack."""
import argparse
import json
import os
import socket
import subprocess
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import urlopen

from e3_run import MODES
from e4_preflight import hardware_metadata, mode_order, policy_manifest, validate_receipt, validate_receipt_chain, verify_topology

REPO = Path(__file__).resolve().parents[1]
AXES = {"seed": 1, "np": 4, "shipment": 16, "epoch_s": 60, "epochs": 1}


def validate_local_result(record):
    measured = record["measurement"]
    mode = record["mode"]
    workload = record["trustedWorkload"]
    if record.get("validationKind") != "local-smoke" or measured.get("validationKind") != "local-smoke":
        raise ValueError("E3 local result needs explicit local-smoke provenance")
    if measured.get("status") != "ok" or measured.get("pacing") != "accelerated":
        raise ValueError("E3 local mode did not complete accelerated execution")
    if (measured.get("sourceRevision") != record["git_commit"]
            or measured.get("registryManifestSHA256") != record["registry_manifest_sha256"]):
        raise ValueError("E3 local binary source/manifest provenance differs")
    if (measured.get("workload_digest", "").lower() != workload["digest"].lower()
            or measured.get("offered_events") != workload["offeredEvents"]
            or measured.get("offered_shipments") != workload["offeredShipments"]):
        raise ValueError("E3 local workload differs from the trusted paired schedule")
    if (measured.get("epochs_completed") != 1 or measured.get("queue_drained") is not True
            or measured.get("committed_events") != measured.get("offered_events")
            or measured.get("committed_shipments") != measured.get("offered_shipments")
            or not 8 <= measured.get("committed_events", 0) <= 16
            or measured.get("committed_shipments") != 1):
        raise ValueError("E3 local mode has incomplete event/epoch/shipment accounting")
    receipts = measured.get("transactions", [])
    expected_receipts = 0 if mode == "fabric-only" else 1
    if len(receipts) != expected_receipts:
        raise ValueError("E3 local mode has missing or extra Anvil receipts")
    if mode == "fabric-only" and (measured.get("gas_samples") or measured.get("calldata_samples")):
        raise ValueError("Fabric-only has no on-chain gas or calldata")
    if mode != "fabric-only" and (len(measured.get("gas_samples", [])) != expected_receipts
                                    or len(measured.get("calldata_samples", [])) != expected_receipts):
        raise ValueError("E3 local L1 mode has missing gas or calldata samples")
    if len(measured.get("audit_samples", [])) != 1:
        raise ValueError("E3 local mode needs its one-shipment audit sample")
    for receipt in receipts:
        validate_receipt(receipt, proof=mode in ("vecro-adapted", "proposed"))
        if mode in ("vecro-adapted", "proposed"):
            pair = {key: receipt[key].lower() for key in ("certificateRoot", "polygonCommitment")}
            if pair not in record["acceptedPolicies"]:
                raise ValueError("E3 local receipt policy roots differ from trusted manifest")
    if mode in ("vecro-adapted", "proposed"):
        validate_receipt_chain(receipts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--gate", type=Path, required=True)
    parser.add_argument("--policy-manifest", type=Path, required=True)
    parser.add_argument("--mode", choices=MODES, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--allocation-root", type=Path, required=True)
    parser.add_argument("--wall-time-s", type=int, default=7200)
    args = parser.parse_args()
    root = args.allocation_root.resolve()
    if (args.out.exists() or not args.out.is_absolute() or args.out.resolve().is_relative_to(REPO)
            or not args.out.resolve().is_relative_to(root) or args.wall_time_s < 600
            or not args.binary.is_file()):
        parser.error("new private output, allocation root, release binary and >=600 s budget required")
    owner = root / "stack-owner"
    if not owner.is_file() or owner.read_text().strip() != str(REPO):
        parser.error("E3 local mode requires the allocation-owned live stack")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO):
        parser.error("E3 local mode requires a clean source checkout")
    gate = json.loads(args.gate.read_text())
    if (gate.get("protocolVersion") != 2 or gate.get("status") != "passed"
            or gate.get("verificationKind") != "sp1-groth16" or gate.get("gitCommit") != revision
            or gate.get("anvilReceiptStatus") != 1 or gate.get("fabricEventsReadback") != 16
            or not gate.get("transactionHash")):
        parser.error("same-revision real E3 aggregate gate must pass before local paired smoke")
    manifest_bytes = args.policy_manifest.read_bytes()
    manifest = json.loads(manifest_bytes)
    digest, policies = policy_manifest(args.policy_manifest)
    selected = [row for row in manifest.get("workloads", []) if all(row.get(key) == value for key, value in AXES.items())]
    if (manifest.get("validationKind") != "local-smoke" or manifest.get("sourceRevision") != revision
            or len(selected) != 1 or not 8 <= selected[0].get("offeredEvents", 0) <= 16
            or selected[0].get("offeredShipments") != 1
            or manifest.get("leafVKeyDigest") != gate.get("policyProgramVKeyDigest")
            or manifest.get("aggregateVKeyDigest") != gate.get("aggregateProgramVKeyDigest")):
        parser.error("trusted local paired workload/keys do not match the passed E3 gate")
    topology = verify_topology("http://127.0.0.1:8080", "http://127.0.0.1:8545")
    anvil = json.loads(subprocess.check_output(["docker", "inspect", "e2-anvil"], text=True))[0]
    if not anvil["State"]["Running"]:
        parser.error("Anvil is not running")
    try:
        with urlopen("http://127.0.0.1:8080/events/200000", timeout=10):
            parser.error("measured event already exists; ledger is not fresh for this mode")
    except HTTPError as error:
        if error.code != 404 or b"does not exist" not in error.read():
            raise
    command = [str(args.binary.resolve()), "--local-smoke", "--mode", args.mode,
               "--seed", "1", "--np", "4", "--shipment", "16", "--epoch-s", "60",
               "--epochs", "1", "--prover", "cpu", "--wall-time-s", str(args.wall_time_s)]
    args.out.parent.mkdir(parents=True, exist_ok=True)
    try:
        run = subprocess.run(command, cwd=REPO, env=dict(os.environ, POLICY_MANIFEST=str(args.policy_manifest.resolve()),
                             RAYON_NUM_THREADS="2"), text=True, capture_output=True, timeout=args.wall_time_s + 120)
        args.out.with_suffix(".stdout.log").write_text(run.stdout)
        args.out.with_suffix(".stderr.log").write_text(run.stderr)
        measured = json.loads(run.stdout.strip().splitlines()[-1]) if run.returncode == 0 else {
            "status": "censored", "censor_reason": f"binary exit {run.returncode}: {run.stderr[-500:]}"}
    except subprocess.TimeoutExpired as error:
        measured = {"status": "saturated", "censor_reason": f"timeout after {error.timeout} seconds"}
    hardware = hardware_metadata()
    hardware["gpu_model_driver"] = hardware["gpu_model_driver"] or "N/A"
    hardware["cuda_toolkit"] = hardware["cuda_toolkit"] or "N/A"
    record = {
        "experiment": "E3", "protocolVersion": 2, "validationKind": "local-smoke",
        "git_commit": revision, "gate_transaction_hash": gate["transactionHash"],
        "registry_manifest_sha256": digest, "acceptedPolicies": policies,
        "trustedWorkload": selected[0], "mode": args.mode, "seed": 1, "prover": "cpu",
        "mode_order": mode_order(1), "mode_order_index": mode_order(1).index(args.mode),
        "np": 4, "lambda_events_per_min": 15.36, "duration_min": 1,
        "shipment": 16, "epoch_s": 60, "epochs_target": 1,
        "hardware": hardware, "software": {"sp1_version": "6.4.0", "fabric_images": topology["fabricContainers"],
            "anvil_version": topology["anvilClientVersion"]},
        "ledgerIsolation": {"protocolVersion": 2, "freshLedger": True, "sourceRevision": revision,
            "mode": args.mode, "seed": 1, "computeHost": socket.gethostname(), "order": mode_order(1).index(args.mode),
            "runtimePath": str(root), "namespace": f"e3-local-{args.mode}-{anvil['Id'][:12]}",
            "anvilContainerId": anvil["Id"], "firstMeasuredEventAbsentBeforeRun": True},
        "measurement": measured,
    }
    if measured.get("status") == "ok":
        validate_local_result(record)
    with args.out.open("x") as output:
        json.dump(record, output, indent=2)
        output.write("\n")
    print(f"E3 local {args.mode}: {measured.get('status')} -> {args.out}")
    if measured.get("status") != "ok":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
