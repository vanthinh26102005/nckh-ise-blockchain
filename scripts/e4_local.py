#!/usr/bin/env python3
"""Immutable four-epoch CPU E4 smoke shards, merge gate and local report.

This path is deliberately separate from the 80+270 formal CPU/GPU sweep.
Raw proofs, witnesses and Fabric state stay outside Git.
"""
import argparse
import hashlib
import json
import math
import re
import socket
import statistics
from pathlib import Path

from e4_preflight import validate_receipt_chain

REPO = Path(__file__).resolve().parents[1]
AXES = ("seed", "np", "shipment", "epoch_s", "epochs")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fresh_out(path):
    path = path.resolve()
    if path.is_relative_to(REPO) or path.exists():
        raise ValueError("local raw output must be fresh and outside Git")
    return path


def check_run(run, policy, gate, policy_sha):
    if run.get("validationKind") != "e4-local-smoke" or run.get("pacing") != "accelerated" or run.get("mode") != "proposed":
        raise ValueError("E4 local result needs the bounded proposed CPU path")
    if (gate.get("status") != "passed" or gate.get("anvilReceiptStatus") != 1
            or gate.get("protocolVersion") != 2 or gate.get("verificationKind") != "sp1-groth16"
            or gate.get("tamperedAggregateProofRejected") is not True
            or gate.get("tamperedAggregateValuesRejected") is not True
            or gate.get("gitCommit") != run.get("sourceRevision")):
        raise ValueError("E3 gate does not pass on this source revision")
    if policy.get("sourceRevision") != run.get("sourceRevision") or run.get("registryManifestSHA256") != policy_sha:
        raise ValueError("source or trusted registry manifest mismatch")
    if run.get("host") != socket.gethostname():
        raise ValueError("run was produced on a different local host")
    if run.get("protocolVersion") != 2 or run.get("verificationKind") != "sp1-groth16":
        raise ValueError("real policy-v2 SP1 verification is required")
    if run.get("status") != "ok" or run.get("epochs_completed") != 4 or run.get("queue_drained") is not True:
        raise ValueError("run is censored, saturated or has fewer than four drained epochs")
    workloads = policy.get("workloads", [])
    if len(workloads) != 1:
        raise ValueError("one trusted local workload is required")
    workload = workloads[0]
    if (workload.get("epochs"), workload.get("shipment"), workload.get("np"), workload.get("epoch_s")) != (4, 16, 3, 60):
        raise ValueError("unexpected bounded E4 local axes")
    if workload.get("seed") != 3:
        raise ValueError("unexpected E4 local seed")
    if workload.get("digest", "").lower() != run.get("workload_digest", "").lower():
        raise ValueError("trusted workload digest mismatch")
    if any(run.get(measured) != workload.get(trusted) for measured, trusted in
           (("offered_events", "offeredEvents"), ("offered_shipments", "offeredShipments"))):
        raise ValueError("trusted workload counts mismatch")
    if run.get("committed_events") != run.get("offered_events") or run.get("committed_shipments") != run.get("offered_shipments"):
        raise ValueError("queue did not drain all events and shipments")
    if run.get("offered_shipments") != 4:
        raise ValueError("local configuration needs one shipment per epoch")
    pair = {key: run.get("registryPair", {}).get(key, "").lower() for key in ("certificateRoot", "polygonCommitment")}
    accepted = accepted_policy_pairs(policy)
    if pair not in accepted:
        raise ValueError("E4 workload registry roots are not trusted")
    for key in ("leafVKeyDigest", "aggregateVKeyDigest"):
        gate_key = "policyProgramVKeyDigest" if key == "leafVKeyDigest" else "aggregateProgramVKeyDigest"
        if str(policy.get(key, "")).lower() != str(gate.get(gate_key, "")).lower():
            raise ValueError("E3 gate and E4 verification keys mismatch")
    epochs = run.get("epoch_samples", [])
    receipts = run.get("transactions", [])
    if len(epochs) != 4 or len(receipts) != 4:
        raise ValueError("four epoch samples and four mined receipts required")
    validate_receipt_chain(receipts)
    if len({int(receipt["epochId"]) for receipt in receipts}) != 4:
        raise ValueError("duplicate epoch ID")
    if any(len(run.get(field, [])) != 4 for field in ("leaf_samples", "aggregate_samples", "gas_samples", "calldata_samples")):
        raise ValueError("one proof and mined transaction per epoch required")
    if len(run.get("audit_samples", [])) != len(run["leaf_samples"]):
        raise ValueError("audit latency needs one sample per committed leaf")
    if len(workload.get("perEpochEvents", [])) != 4 or len(workload.get("perEpochShipments", [])) != 4:
        raise ValueError("trusted workload needs four epoch counts")
    for index, (epoch, receipt) in enumerate(zip(epochs, receipts)):
        if (epoch.get("events") != workload["perEpochEvents"][index] or epoch.get("shipments") != 1
                or workload["perEpochShipments"][index] != 1 or epoch.get("lots") != 1
                or not 8 <= epoch["events"] <= 16):
            raise ValueError(f"epoch {index} event, shipment or lot accounting mismatch")
        if (str(epoch.get("epochId")) != str(receipt.get("epochId"))
                or str(epoch.get("events")) != str(receipt.get("eventCount"))
                or str(epoch.get("lots")) != str(receipt.get("lotCount"))
                or epoch.get("transactionHash") != receipt.get("transactionHash")):
            raise ValueError(f"epoch {index} differs from its mined receipt")
        if (epoch.get("leafProvingMs") != [run["leaf_samples"][index]]
                or epoch.get("aggregateProvingMs") != run["aggregate_samples"][index]
                or epoch.get("gasUsed") != run["gas_samples"][index]
                or epoch.get("calldataBytes") != run["calldata_samples"][index]):
            raise ValueError(f"epoch {index} samples differ from run")
        if len(epoch.get("auditLatencyMs", [])) != epoch["lots"]:
            raise ValueError(f"epoch {index} audit samples incomplete")
        for key in pair:
            if receipt.get(key, "").lower() != pair[key]:
                raise ValueError("receipt uses a different registry pair")
        if receipt.get("programVKey", "").lower() != policy.get("aggregateProgramVKey", "").lower():
            raise ValueError("receipt aggregate key differs from trusted policy")
        if any(receipt.get(key, "").lower() != policy.get(key, "").lower()
               for key in ("leafVKeyDigest", "aggregateVKeyDigest")):
            raise ValueError("receipt verification-key digests differ from trusted policy")
    if sum(epoch["events"] for epoch in epochs) != run["offered_events"]:
        raise ValueError("per-epoch events do not sum to offered count")
    if [value for epoch in epochs for value in epoch["auditLatencyMs"]] != run["audit_samples"]:
        raise ValueError("per-epoch audit samples differ from run")
    if not isinstance(run.get("elapsed_s"), (int, float)) or not math.isfinite(run["elapsed_s"]) or run["elapsed_s"] <= 0:
        raise ValueError("measured wall elapsed time is required")
    return workload


def check_partial_run(run, policy, gate, policy_sha):
    """Keep only completed, verifiable epochs when a real run stops early."""
    if run.get("status") not in ("censored", "saturated") or not run.get("censor_reason") or run.get("queue_drained") is not False:
        raise ValueError("partial E4 needs a censored/saturated status and reason")
    if run.get("validationKind") != "e4-local-smoke" or run.get("mode") != "proposed" or run.get("pacing") != "accelerated":
        raise ValueError("partial E4 must come from the local proposed path")
    if (gate.get("status") != "passed" or gate.get("anvilReceiptStatus") != 1
            or gate.get("protocolVersion") != 2 or gate.get("verificationKind") != "sp1-groth16"
            or gate.get("tamperedAggregateProofRejected") is not True
            or gate.get("tamperedAggregateValuesRejected") is not True
            or gate.get("gitCommit") != run.get("sourceRevision")
            or policy.get("sourceRevision") != run.get("sourceRevision")
            or run.get("registryManifestSHA256") != policy_sha
            or run.get("host") != socket.gethostname()):
        raise ValueError("partial E4 source, gate, host or manifest mismatch")
    if run.get("protocolVersion") != 2 or run.get("verificationKind") != "sp1-groth16":
        raise ValueError("partial E4 requires real policy-v2 verification")
    workloads = policy.get("workloads", [])
    if len(workloads) != 1:
        raise ValueError("partial E4 needs one trusted workload")
    workload = workloads[0]
    if tuple(workload.get(key) for key in AXES) != (3, 3, 16, 60, 4):
        raise ValueError("partial E4 axes mismatch")
    if (run.get("workload_digest", "").lower() != workload.get("digest", "").lower()
            or run.get("offered_events") != workload.get("offeredEvents")
            or run.get("offered_shipments") != workload.get("offeredShipments")):
        raise ValueError("partial E4 trusted workload mismatch")
    pair = {key: run.get("registryPair", {}).get(key, "").lower() for key in ("certificateRoot", "polygonCommitment")}
    if pair not in accepted_policy_pairs(policy):
        raise ValueError("partial E4 registry roots are not trusted")
    for key, gate_key in (("leafVKeyDigest", "policyProgramVKeyDigest"), ("aggregateVKeyDigest", "aggregateProgramVKeyDigest")):
        if str(policy.get(key, "")).lower() != str(gate.get(gate_key, "")).lower():
            raise ValueError("partial E4 verification keys differ from gate")
    epochs = run.get("epoch_samples", [])
    receipts = run.get("transactions", [])
    completed = run.get("epochs_completed")
    if type(completed) is not int or not 0 <= completed <= 4 or len(epochs) != completed or len(receipts) != completed:
        raise ValueError("partial E4 completed-epoch accounting mismatch")
    validate_receipt_chain(receipts)
    if len(workload.get("perEpochEvents", [])) != 4 or len(workload.get("perEpochShipments", [])) != 4:
        raise ValueError("partial E4 trusted per-epoch counts missing")
    for index, (epoch, receipt) in enumerate(zip(epochs, receipts)):
        if (epoch.get("events") != workload["perEpochEvents"][index]
                or epoch.get("shipments") != 1 or workload["perEpochShipments"][index] != 1
                or epoch.get("lots") != 1
                or str(epoch.get("epochId")) != str(receipt.get("epochId"))
                or str(epoch.get("events")) != str(receipt.get("eventCount"))
                or epoch.get("transactionHash") != receipt.get("transactionHash")
                or len(epoch.get("auditLatencyMs", [])) != 1):
            raise ValueError(f"partial E4 completed epoch {index} is inconsistent")
        if any(receipt.get(key, "").lower() != pair[key] for key in pair):
            raise ValueError("partial E4 receipt registry pair mismatch")
        if receipt.get("programVKey", "").lower() != policy.get("aggregateProgramVKey", "").lower():
            raise ValueError("partial E4 aggregate key mismatch")
        if any(receipt.get(key, "").lower() != policy.get(key, "").lower()
               for key in ("leafVKeyDigest", "aggregateVKeyDigest")):
            raise ValueError("partial E4 receipt verification-key digests mismatch")
    if (run.get("committed_events") != sum(epoch["events"] for epoch in epochs)
            or run.get("committed_shipments") != completed):
        raise ValueError("partial E4 committed counts differ from completed epochs")
    return workload


def accepted_policy_pairs(policy):
    pairs = policy.get("acceptedPolicies", [{key: policy.get(key) for key in ("certificateRoot", "polygonCommitment")}])
    if policy.get("protocolVersion") != 2 or not pairs:
        raise ValueError("trusted policy-v2 registry is required")
    for pair in pairs:
        if any(not isinstance(pair.get(key), str) or not re.fullmatch(r"0x[0-9a-fA-F]{64}", pair[key])
               for key in ("certificateRoot", "polygonCommitment")):
            raise ValueError("trusted registry roots must be bytes32")
    return [{key: pair[key].lower() for key in ("certificateRoot", "polygonCommitment")} for pair in pairs]


def make_shards(run, policy, gate, policy_sha):
    workload = (check_run(run, policy, gate, policy_sha) if run.get("status") == "ok"
                else check_partial_run(run, policy, gate, policy_sha))
    common = {"experiment": "E4", "validationKind": "e4-local-smoke", "formalComplete": False,
              "sourceRevision": run["sourceRevision"], "host": run["host"], "policyManifestSHA256": policy_sha,
              "workloadDigest": run["workload_digest"], "seed": workload["seed"], "np": workload["np"],
              "shipment": workload["shipment"], "epoch_s": workload["epoch_s"], "epochs_target": 4}
    rows = []
    for index, (epoch, receipt) in enumerate(zip(run["epoch_samples"], run["transactions"])):
        rows.append({**common, "epoch_index": index, "epochId": epoch["epochId"], "events": epoch["events"],
                     "shipments": epoch["shipments"], "lots": epoch["lots"], "leafProvingMs": epoch["leafProvingMs"],
                     "aggregateProvingMs": epoch["aggregateProvingMs"], "auditLatencyMs": epoch["auditLatencyMs"],
                     "gasUsed": epoch["gasUsed"], "calldataBytes": epoch["calldataBytes"], "receipt": receipt})
    manifest = {**common, "status": run["status"], "queue_drained": run["queue_drained"],
                "censor_reason": run.get("censor_reason"), "epochs_completed": run["epochs_completed"],
                "elapsed_s": run["elapsed_s"], "offered_events": run["offered_events"],
                "committed_events": run["committed_events"], "offered_shipments": run["offered_shipments"],
                "committed_shipments": run["committed_shipments"], "gateTransactionHash": gate["transactionHash"],
                "gpuCuda": None, "gpuCudaReason": "Apple M1 Pro local CPU; CUDA GPU preflight unavailable"}
    return rows, manifest


def merge(shards, manifest, policy, gate, policy_sha):
    if len(shards) != 4 or sorted(row.get("epoch_index") for row in shards) != list(range(4)):
        raise ValueError("exactly four unique contiguous epoch shards required")
    if manifest.get("status") != "ok" or manifest.get("queue_drained") is not True:
        raise ValueError("partial or censored local run cannot merge as complete")
    if manifest.get("policyManifestSHA256") != policy_sha or manifest.get("sourceRevision") != gate.get("gitCommit"):
        raise ValueError("merged policy, source or gate mismatch")
    if manifest.get("host") != socket.gethostname() or manifest.get("validationKind") != "e4-local-smoke":
        raise ValueError("merged shards are not from this local E4 host")
    ordered = sorted(shards, key=lambda row: row["epoch_index"])
    for row in ordered:
        for key in ("sourceRevision", "host", "policyManifestSHA256", "workloadDigest", "seed", "np", "shipment", "epoch_s", "validationKind"):
            if row.get(key) != manifest.get(key):
                raise ValueError(f"epoch shard {row['epoch_index']} {key} mismatch")
    receipts = [row["receipt"] for row in ordered]
    validate_receipt_chain(receipts)
    if len({r["transactionHash"] for r in receipts}) != 4:
        raise ValueError("duplicate Anvil receipt")
    if [int(r["epochId"]) for r in receipts] != [ordered[0]["epochId"] + i for i in range(4)]:
        raise ValueError("epoch IDs are not contiguous")
    for index, row in enumerate(ordered):
        if (row["events"] <= 0 or row["shipments"] != 1 or row["lots"] != 1
                or str(row["events"]) != str(row["receipt"]["eventCount"])
                or str(row["lots"]) != str(row["receipt"]["lotCount"])
                or len(row["auditLatencyMs"]) != row["lots"]):
            raise ValueError(f"epoch {index} count mismatch")
    run = {"validationKind": manifest["validationKind"], "pacing": "accelerated", "mode": "proposed",
           "sourceRevision": manifest["sourceRevision"], "registryManifestSHA256": policy_sha,
           "host": manifest["host"], "protocolVersion": 2, "verificationKind": "sp1-groth16",
           "status": "ok", "epochs_completed": 4, "queue_drained": True,
           "workload_digest": manifest["workloadDigest"], "offered_events": manifest["offered_events"],
           "committed_events": manifest["committed_events"], "offered_shipments": manifest["offered_shipments"],
           "committed_shipments": manifest["committed_shipments"], "elapsed_s": manifest["elapsed_s"],
           "registryPair": {key: ordered[0]["receipt"][key] for key in ("certificateRoot", "polygonCommitment")},
           "epoch_samples": [{"epochId": r["epochId"], "events": r["events"], "shipments": r["shipments"], "lots": r["lots"],
                              "leafProvingMs": r["leafProvingMs"], "aggregateProvingMs": r["aggregateProvingMs"],
                              "auditLatencyMs": r["auditLatencyMs"], "gasUsed": r["gasUsed"],
                              "calldataBytes": r["calldataBytes"], "transactionHash": r["receipt"]["transactionHash"]} for r in ordered],
           "transactions": receipts, "leaf_samples": [x for r in ordered for x in r["leafProvingMs"]],
           "aggregate_samples": [r["aggregateProvingMs"] for r in ordered],
           "audit_samples": [x for r in ordered for x in r["auditLatencyMs"]],
           "gas_samples": [r["gasUsed"] for r in ordered], "calldata_samples": [r["calldataBytes"] for r in ordered]}
    check_run(run, policy, gate, policy_sha)
    return run


def summarize(run):
    def stats(values):
        return {"mean": statistics.fmean(values), "median": statistics.median(values),
                "min": min(values), "max": max(values)} if values else None
    events, shipments, seconds = run["committed_events"], run["committed_shipments"], run["elapsed_s"]
    gas = sum(run["gas_samples"])
    calldata = sum(run["calldata_samples"])
    return {"experiment": "E4", "validationKind": "e4-local-smoke", "formalComplete": False,
            "gpuCuda": None, "gpuCudaReason": "Apple M1 Pro local CPU; CUDA GPU preflight unavailable",
            "sourceRevision": run["sourceRevision"], "host": run["host"], "epochs": 4,
            "events": events, "shipments": shipments, "lots": len(run["leaf_samples"]),
            "completion_rate": events / run["offered_events"],
            "throughput_events_per_s": events / seconds, "throughput_shipments_per_min": shipments * 60 / seconds,
            "elapsed_s": seconds, "leaf_proving_ms": stats(run["leaf_samples"]),
            "aggregate_proving_ms": stats(run["aggregate_samples"]),
            "audit_latency_ms": stats(run["audit_samples"]), "gas_total": gas,
            "gas_per_shipment": gas / shipments, "calldata_bytes_total": calldata,
            "calldata_bytes_per_shipment": calldata / shipments,
            "receipts": [{"epochId": r["epochId"], "transactionHash": r["transactionHash"],
                          "status": r["status"], "eventCount": r["eventCount"], "lotCount": r["lotCount"],
                          "oldNullifierRoot": r["oldNullifierRoot"], "newNullifierRoot": r["newNullifierRoot"]} for r in run["transactions"]]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("shard", "merge", "analyze"))
    parser.add_argument("--run", type=Path, help="Rust JSON output (shard only)")
    parser.add_argument("--policy", type=Path, help="trusted policy manifest (shard/merge)")
    parser.add_argument("--gate", type=Path, help="same-revision E3 gate manifest (shard/merge)")
    parser.add_argument("--shards", type=Path, help="raw shard directory (shard/merge)")
    parser.add_argument("--merged", type=Path, help="merged JSON output (merge/analyze)")
    parser.add_argument("--report", type=Path, help="report JSON output (analyze)")
    args = parser.parse_args()
    try:
        if args.command in ("shard", "merge"):
            policy_bytes = args.policy.read_bytes()
            policy, gate = json.loads(policy_bytes), json.loads(args.gate.read_text())
            policy_sha = sha(policy_bytes)
        if args.command == "shard":
            out = fresh_out(args.shards)
            run = json.loads(args.run.read_text().splitlines()[-1])
            rows, manifest = make_shards(run, policy, gate, policy_sha)
            out.mkdir(parents=True)
            (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            if run["status"] != "ok":
                (out / "partial-status.json").write_text(json.dumps({"status": run["status"],
                    "censor_reason": run.get("censor_reason"), "epochs_completed": run["epochs_completed"],
                    "offered_events": run["offered_events"], "committed_events": run["committed_events"],
                    "observed_receipts": len(rows), "observed_gas": sum(row["gasUsed"] for row in rows),
                    "observed_calldata_bytes": sum(row["calldataBytes"] for row in rows),
                    "full_run_throughput_events_per_s": None, "full_run_completion_rate": None,
                    "formalComplete": False}, indent=2) + "\n")
            for row in rows:
                folder = out / f"epoch-{row['epoch_index']}"
                folder.mkdir()
                (folder / "raw.jsonl").write_text(json.dumps(row) + "\n")
            print(f"E4 local: wrote {len(rows)} immutable epoch shards to {out} (status={run['status']})")
        elif args.command == "merge":
            out = fresh_out(args.merged)
            manifest = json.loads((args.shards / "manifest.json").read_text())
            paths = sorted(args.shards.rglob("raw.jsonl"))
            rows = []
            for path in paths:
                lines = [line for line in path.read_text().splitlines() if line.strip()]
                if len(lines) != 1:
                    raise ValueError("each epoch shard must contain one raw record")
                rows.append(json.loads(lines[0]))
            run = merge(rows, manifest, policy, gate, policy_sha)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(json.dumps(run, indent=2) + "\n")
            print(f"E4 local: merged four real epochs to {out}")
        else:
            out = fresh_out(args.report)
            run = json.loads(args.merged.read_text())
            if run.get("validationKind") != "e4-local-smoke" or run.get("status") != "ok" or len(run.get("transactions", [])) != 4:
                raise ValueError("analyzer needs a complete local four-epoch merge")
            report = summarize(run)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(json.dumps(report, indent=2) + "\n")
            print(f"E4 local: summarized four epochs to {out}")
    except (ValueError, KeyError, TypeError, OSError, json.JSONDecodeError) as exc:
        parser.error(str(exc))


if __name__ == "__main__":
    main()
