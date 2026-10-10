#!/usr/bin/env python3
"""Audit a one-seed paired E3 CPU smoke; never emit formal statistics."""
import argparse
import json
import math
from pathlib import Path

from e3_local_run import validate_local_result
from e3_run import MODES
from e4_preflight import mode_order

REPO = Path(__file__).resolve().parents[1]


def summarize(records):
    by_mode = {}
    reasons = []
    revisions, gates, manifests, digests, hosts, stacks = (set() for _ in range(6))
    for record in records:
        mode = record.get("mode")
        if mode not in MODES or mode in by_mode:
            raise ValueError("E3 local needs each known mode at most once")
        if (record.get("protocolVersion") != 2 or record.get("validationKind") != "local-smoke"
                or record.get("seed") != 1 or record.get("prover") != "cpu"
                or record.get("mode_order") != list(mode_order(1)) and record.get("mode_order") != mode_order(1)
                or record.get("mode_order_index") != mode_order(1).index(mode)):
            raise ValueError("E3 local mode metadata/paired order is invalid")
        isolation = record.get("ledgerIsolation", {})
        if (isolation.get("freshLedger") is not True or isolation.get("firstMeasuredEventAbsentBeforeRun") is not True
                or isolation.get("sourceRevision") != record.get("git_commit")
                or isolation.get("mode") != mode or isolation.get("seed") != 1
                or isolation.get("computeHost") != record.get("hardware", {}).get("hostname")
                or isolation.get("order") != mode_order(1).index(mode)
                or not isolation.get("anvilContainerId")):
            raise ValueError("E3 local mode lacks fresh-ledger/host/ordering evidence")
        revisions.add(record.get("git_commit"))
        gates.add(record.get("gate_transaction_hash"))
        manifests.add(record.get("registry_manifest_sha256"))
        digests.add(record.get("trustedWorkload", {}).get("digest"))
        hosts.add(record.get("hardware", {}).get("hostname"))
        if isolation["anvilContainerId"] in stacks:
            raise ValueError("E3 local paired modes reused an Anvil/Fabric stack")
        stacks.add(isolation["anvilContainerId"])
        measured = record.get("measurement", {})
        if measured.get("status") == "ok":
            validate_local_result(record)
        else:
            reasons.append(f"{mode}: {measured.get('status', 'missing terminal status')}")
        by_mode[mode] = record
    if any(len(values) != 1 or None in values for values in (revisions, gates, manifests, digests, hosts)):
        raise ValueError("E3 local paired modes differ in source, gate, manifest, workload or host")
    missing = [mode for mode in MODES if mode not in by_mode]
    if missing:
        reasons.append("missing paired modes: " + ", ".join(missing))
    metrics = {}
    for mode in MODES:
        if mode not in by_mode or by_mode[mode]["measurement"].get("status") != "ok":
            metrics[mode] = None
            continue
        measured = by_mode[mode]["measurement"]
        seconds = measured["elapsed_s"]
        shipments = measured["committed_shipments"]
        if not isinstance(seconds, (float, int)) or not math.isfinite(seconds) or seconds <= 0 or shipments != 1:
            raise ValueError("E3 local completed mode has invalid elapsed time or shipment count")
        audit = measured["audit_samples"]
        gas = measured.get("gas_samples", [])
        calldata = measured.get("calldata_samples", [])
        if not audit or any(not isinstance(value, (float, int)) or not math.isfinite(value) or value < 0
                            for value in [*audit, *gas, *calldata]):
            raise ValueError("E3 local measurement samples must be finite and nonnegative")
        metrics[mode] = {
            "elapsedSeconds": seconds,
            "throughputShipmentsPerMin": shipments * 60 / seconds,
            "throughputEventsPerSec": measured["committed_events"] / seconds,
            "auditLatencyMs": audit,
            "gasPerShipment": None if mode == "fabric-only" else sum(gas) / shipments,
            "calldataBytesPerShipment": None if mode == "fabric-only" else sum(calldata) / shipments,
            "transactions": [row["transactionHash"] for row in measured.get("transactions", [])],
        }
    return {
        "experiment": "E3 local paired CPU smoke", "protocolVersion": 2,
        "validationKind": "local-smoke", "localComplete": not reasons,
        "formalComplete": False, "evidenceClass": "local-smoke" if not reasons else "local-smoke-incomplete",
        "nonformalReasons": ["one accelerated CPU seed; formal 4 × 30 paced GPU/HPC runs pending", *reasons],
        "seed": 1, "modeOrder": mode_order(1), "sourceRevision": next(iter(revisions)),
        "gateTransactionHash": next(iter(gates)), "manifestSha256": next(iter(manifests)),
        "workloadDigest": next(iter(digests)), "host": next(iter(hosts)),
        "runs": len(records), "modes": metrics, "pairedTests": [],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists() or not args.out.is_absolute() or args.out.resolve().is_relative_to(REPO):
        parser.error("new absolute analysis output outside Git required")
    records = [json.loads(path.read_text()) for path in sorted(args.raw_dir.glob("*.json"))]
    if not records:
        parser.error("no local E3 mode records found")
    result = summarize(records)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(f"E3 localComplete={result['localComplete']} modes={len(records)} -> {args.out}")


if __name__ == "__main__":
    main()
