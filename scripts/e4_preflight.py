#!/usr/bin/env python3
"""Run the E4 CPU/GPU A/B gate on one Fabric + Anvil topology."""
import argparse
import hashlib
import math
import platform
import re
import json
import os
import socket
import subprocess
import tomllib
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen


FABRIC_CONTAINERS = (
    "e2-orderer1.example.com", "e2-orderer2.example.com", "e2-orderer3.example.com",
    "e2-peer0.org1.example.com", "e2-peer0.org2.example.com",
)


def verify_topology(fabric_gateway, anvil_rpc):
    inspected = json.loads(subprocess.run(["docker", "inspect", *FABRIC_CONTAINERS],
                                          text=True, capture_output=True, check=True).stdout)
    if len(inspected) != len(FABRIC_CONTAINERS):
        raise RuntimeError("Fabric topology has missing containers")
    containers = {}
    for item in inspected:
        name = item["Name"].lstrip("/")
        if name not in FABRIC_CONTAINERS or not item["State"]["Running"]:
            raise RuntimeError(f"Fabric container {name} is not in the required running topology")
        containers[name] = {"id": item["Id"], "image": item["Config"]["Image"]}
    if set(containers) != set(FABRIC_CONTAINERS):
        raise RuntimeError("Fabric topology differs from the required 2 peer / 3 Raft setup")
    with urlopen(fabric_gateway + "/health", timeout=10) as response:
        if json.load(response).get("status") != "ok":
            raise RuntimeError("Fabric Gateway health check failed")
    request = Request(anvil_rpc, data=b'{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}',
                      headers={"content-type": "application/json"})
    with urlopen(request, timeout=10) as response:
        chain_id = json.load(response).get("result")
    if not chain_id:
        raise RuntimeError("Anvil RPC did not report an Ethereum chain ID")
    request = Request(anvil_rpc, data=b'{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":2}',
                      headers={"content-type": "application/json"})
    with urlopen(request, timeout=10) as response:
        client_version = json.load(response).get("result")
    if not client_version:
        raise RuntimeError("Anvil RPC did not report a client version")
    return {"fabricContainers": containers, "anvilChainId": chain_id,
            "anvilClientVersion": client_version}



def mode_order(seed):
    # Four-period balanced Latin square; 30 seeds differ by at most one at each position.
    rows = ((0, 1, 3, 2), (1, 2, 0, 3), (2, 3, 1, 0), (3, 0, 2, 1))
    modes = ("fabric-only", "hash-on-chain", "vecro-adapted", "proposed")
    return tuple(modes[index] for index in rows[seed % 4])


def policy_manifest(path):
    data = Path(path).read_bytes()
    manifest = json.loads(data)
    if manifest.get("protocolVersion") != 2:
        raise ValueError("trusted policy manifest requires protocolVersion 2")
    pairs = manifest.get("acceptedPolicies", [{"certificateRoot": manifest.get("certificateRoot"), "polygonCommitment": manifest.get("polygonCommitment")}])
    if not isinstance(pairs, list) or not pairs:
        raise ValueError("trusted acceptedPolicies must be nonempty")
    for pair in pairs:
        if any(not isinstance(pair.get(key), str) or not re.fullmatch(r"0x[0-9a-fA-F]{64}", pair[key]) for key in ("certificateRoot", "polygonCommitment")):
            raise ValueError("trusted policy roots must be bytes32")
    return hashlib.sha256(data).hexdigest(), [{key: pair[key].lower() for key in ("certificateRoot", "polygonCommitment")} for pair in pairs]


def hardware_metadata():
    def output(command):
        try:
            return subprocess.check_output(command, stderr=subprocess.DEVNULL, text=True).strip()
        except (OSError, subprocess.CalledProcessError):
            return None
    cpu = platform.processor()
    ram = None
    if Path("/proc/cpuinfo").exists():
        cpu = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), cpu)
    if Path("/proc/meminfo").exists():
        ram = next((int(line.split()[1]) * 1024 for line in Path("/proc/meminfo").read_text().splitlines() if line.startswith("MemTotal:")), None)
    elif platform.system() == "Darwin":
        cpu = output(["sysctl", "-n", "machdep.cpu.brand_string"])
        value = output(["sysctl", "-n", "hw.memsize"])
        ram = int(value) if value else None
    return {"hostname": socket.gethostname(), "cpu_model": cpu or None, "cpu_logical": os.cpu_count(), "ram_bytes": ram, "gpu_model_driver": output(["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"]), "cuda_toolkit": output(["nvcc", "--version"]), "os": platform.platform()}


def validate_receipt(receipt, proof=True):
    if not isinstance(receipt, dict) or receipt.get("status") != 1 or not receipt.get("transactionHash") or not receipt.get("gasUsed"):
        raise ValueError("successful Anvil receipt with transaction hash and gas is required")
    if receipt.get("protocolVersion") != 2:
        raise ValueError("receipt requires policy-v2")
    hex32 = lambda value: isinstance(value, str) and re.fullmatch(r"0x[0-9a-fA-F]{64}", value)
    address = lambda value: isinstance(value, str) and re.fullmatch(r"0x[0-9a-fA-F]{40}", value)
    if not hex32(receipt.get("transactionHash")) or not re.fullmatch(r"[0-9]+", str(receipt.get("gasUsed", ""))) or int(receipt["gasUsed"]) <= 0:
        raise ValueError("receipt transaction hash and gas must be canonical")
    if not proof:
        if receipt.get("verificationKind") != "hash-on-chain":
            raise ValueError("hash baseline receipt must report hash-on-chain")
        event=receipt.get("anchorEvent",{})
        if (not address(receipt.get("anchor")) or event.get("name")!="ShipmentAnchored"
                or str(event.get("address","")).lower()!=receipt["anchor"].lower()
                or str(event.get("transactionHash","")).lower()!=receipt["transactionHash"].lower()
                or not hex32(event.get("blockHash")) or not hex32(receipt.get("digest"))):
            raise ValueError("hash baseline needs its mined ShipmentAnchored event")
        if not str(receipt.get("shipmentId","")).isdigit() or int(receipt["shipmentId"])>=2**64:
            raise ValueError("hash baseline shipmentId must fit uint64")
        if type(receipt.get("eventCount")) is not int or not 1<=receipt["eventCount"]<=128:
            raise ValueError("hash baseline eventCount outside shipment bounds")
        if any(str(event.get("args",{}).get(key,"")).lower()!=str(receipt[key]).lower() for key in ("shipmentId","digest","eventCount")):
            raise ValueError("ShipmentAnchored event args differ from mined shipment receipt")
        return
    if (receipt.get("verificationKind") != "sp1-groth16" or not address(receipt.get("registry"))
            or not address(receipt.get("anchor")) or receipt.get("publicValuesBytes") != 352 or receipt.get("policyValid") is not True):
        raise ValueError("receipt requires real SP1 Groth16, canonical registry/anchor and valid policy metadata")
    for key in ("programVKey", "oldNullifierRoot", "newNullifierRoot", "certificateRoot", "polygonCommitment"):
        if not hex32(receipt.get(key)):
            raise ValueError(f"receipt needs canonical {key}")
    if receipt["oldNullifierRoot"].lower() == receipt["newNullifierRoot"].lower():
        raise ValueError("receipt nullifier root did not advance")
    event = receipt.get("anchorEvent")
    if not isinstance(event, dict) or event.get("name") not in ("EpochAggregated", "EpochAnchored", "TokenMinted"):
        raise ValueError("receipt needs a decoded policy anchor event")
    if (str(event.get("address", "")).lower() != receipt["anchor"].lower()
            or str(event.get("transactionHash", "")).lower() != receipt["transactionHash"].lower() or not hex32(event.get("blockHash"))):
        raise ValueError("anchor event does not belong to the mined anchor transaction")
    args = event.get("args")
    if not isinstance(args, dict):
        raise ValueError("anchor event needs decoded arguments")
    def count(record, key):
        raw = str(record.get(key, ""))
        if not re.fullmatch(r"0|[1-9][0-9]*", raw) or int(raw) >= 2**64:
            raise ValueError(f"anchor metadata needs canonical uint64 {key}")
        return int(raw)
    if count(args, "epochId") != count(receipt, "epochId"):
        raise ValueError("anchor event epoch differs from verified receipt")
    matched = ["epochId"]
    if event["name"] == "EpochAggregated":
        lots, events = count(receipt, "lotCount"), count(receipt, "eventCount")
        if lots <= 0 or not lots * 8 <= events <= lots * 64:
            raise ValueError("aggregate receipt event/lot counts outside policy bounds")
        matched += ["epochCommitment", "oldNullifierRoot", "newNullifierRoot", "lotCount", "eventCount"]
        for key in ("epochCommitment", "leafVKeyDigest", "aggregateVKeyDigest"):
            if not hex32(receipt.get(key)):
                raise ValueError(f"aggregate receipt needs canonical {key}")
    else:
        if not 8 <= count(receipt, "eventCount") <= 64 or receipt.get("role") != 4 or receipt.get("threshold") != 900:
            raise ValueError("leaf receipt event/role/threshold metadata outside policy bounds")
        for key in ("eventBatchDigest", "nullifier"):
            if not hex32(receipt.get(key)):
                raise ValueError(f"leaf receipt needs canonical {key}")
        matched += ["eventBatchDigest"]
        if event["name"] == "EpochAnchored":
            matched += ["oldNullifierRoot", "newNullifierRoot", "nullifier"]
        else:
            if not hex32(args.get("tokenId")):
                raise ValueError("TokenMinted event needs canonical tokenId")
            matched += ["eventCount"]
    for key in matched:
        if str(args.get(key, "")).lower() != str(receipt.get(key, "")).lower():
            raise ValueError(f"anchor event {key} differs from the verified receipt")


def validate_receipt_chain(receipts):
    previous = None
    seen = set()
    for receipt in receipts:
        validate_receipt(receipt)
        transaction = receipt["transactionHash"].lower()
        if transaction in seen:
            raise ValueError("duplicate mined transaction receipt")
        seen.add(transaction)
        if previous is not None:
            if receipt["anchor"].lower() != previous["anchor"].lower() or receipt["registry"].lower() != previous["registry"].lower():
                raise ValueError("proof receipt chain changed anchor or registry")
            if receipt["oldNullifierRoot"].lower() != previous["newNullifierRoot"].lower():
                raise ValueError("proof receipt root chain is not contiguous")
        previous = receipt


def validate_preflight(record):
    if record.get("protocolVersion") != 2 or record.get("verificationKind") != "sp1-groth16":
        raise ValueError("preflight requires policy-v2 real SP1 Groth16")
    if not re.fullmatch(r"[0-9a-f]{64}", record.get("registry_manifest_sha256", "")) or not record.get("acceptedPolicies"):
        raise ValueError("preflight requires trusted registry manifest provenance")
    hardware = record.get("hardware", {})
    if any(not hardware.get(key) for key in ("hostname", "cpu_model", "cpu_logical", "ram_bytes", "gpu_model_driver", "cuda_toolkit", "os")):
        raise ValueError("preflight requires CPU/RAM/GPU/driver/CUDA/OS hardware metadata")
    samples = record.get("samples", [])
    expected = {(events, prover, repetition) for events in (8, 64) for prover in ("cpu", "cuda") for repetition in range(5)}
    actual = {(row.get("events"), row.get("prover"), row.get("repetition")) for row in samples}
    if actual != expected or len(samples) != 20:
        raise ValueError("preflight requires five CPU/GPU repetitions at 8 and 64 events")
    pairing_keys = ("workloadDigest", "leafVKeyDigest", "aggregateVKeyDigest", "oldNullifierRoot", "newNullifierRoot", "certificateRoot", "polygonCommitment")
    for events in (8, 64):
        group = [row for row in samples if row["events"] == events]
        for key in pairing_keys:
            values = {row.get(key) for row in group}
            if len(values) != 1 or None in values or "" in values:
                raise ValueError(f"preflight {events} CPU/GPU input/key/root mismatch: {key}")
        pair = {key: group[0][key].lower() for key in ("certificateRoot", "polygonCommitment")}
        if pair not in record["acceptedPolicies"]:
            raise ValueError("preflight root pair is outside trusted manifest")
    for sample in samples:
        if any(not isinstance(sample.get(key), (int, float)) or not math.isfinite(sample[key]) or sample[key] <= 0 for key in ("leaf_ms", "aggregate_ms")):
            raise ValueError("preflight timings must be positive finite measurements")
        validate_receipt(sample.get("receipt"))
        for key in ("leafVKeyDigest", "aggregateVKeyDigest", "oldNullifierRoot", "newNullifierRoot", "certificateRoot", "polygonCommitment"):
            if sample["receipt"].get(key, "").lower() != sample[key].lower():
                raise ValueError(f"preflight receipt {key} disagrees with proved fixture")
    return record

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True, help="Built e3_epoch_gate binary with CUDA")
    parser.add_argument("--contracts", type=Path, default=Path("contracts"))
    parser.add_argument("--raw-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--epoch-base", type=int, required=True)
    parser.add_argument("--fabric-gateway", default="http://127.0.0.1:8080")
    parser.add_argument("--anvil-rpc", default="http://127.0.0.1:8545")
    parser.add_argument("--git-commit", required=True)
    parser.add_argument("--policy-manifest", type=Path, required=True)
    args = parser.parse_args()
    if not args.binary.is_file():
        parser.error("E3 gate binary is missing")
    if args.raw_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1]):
        parser.error("preflight proof fixtures must be outside the Git repository")
    if args.out.exists() or (args.raw_dir.exists() and any(args.raw_dir.iterdir())):
        parser.error("preflight outputs already exist; use fresh paths to preserve evidence")
    local = {"localhost", "127.0.0.1", "::1"}
    if urlparse(args.fabric_gateway).hostname not in local or urlparse(args.anvil_rpc).hostname not in local:
        parser.error("formal preflight requires Fabric and Anvil on the compute host")
    manifest_sha, policies = policy_manifest(args.policy_manifest)
    topology_evidence = verify_topology(args.fabric_gateway, args.anvil_rpc)
    lock = tomllib.loads((Path(__file__).resolve().parents[1] / "crates/sp1-e2e/Cargo.lock").read_text())
    sp1_version = next(package["version"] for package in lock["package"] if package["name"] == "sp1-sdk")
    args.raw_dir.mkdir(parents=True, exist_ok=True)
    timings = {}
    receipts = {}
    samples = []
    for events in (8, 64):
        for repetition in range(5):
            for prover in ("cpu", "cuda") if repetition % 2 == 0 else ("cuda", "cpu"):
                fixture_path = args.raw_dir / f"{prover}-{events}-rep{repetition}-epoch-proof.json"
                command = [str(args.binary.resolve()), "--fabric-gateway", args.fabric_gateway,
                           "--prover", prover, "--events-per-leaf", str(events),
                           "--epoch-id", str(args.epoch_base + (events == 64)), "--out", str(fixture_path)]
                if not (repetition == 0 and prover == "cpu"):
                    command.append("--reuse-fabric-evidence")
                subprocess.run(command, check=True)
                fixture = json.loads(fixture_path.read_text())
                if fixture.get("protocolVersion") != 2 or fixture.get("verificationKind") != "sp1-groth16":
                    raise RuntimeError("preflight fixture must be a real policy-v2 proof")
                env = dict(os.environ, E3_FIXTURE=str(fixture_path.resolve()), ANVIL_RPC=args.anvil_rpc,
                           POLICY_MANIFEST=str(args.policy_manifest.resolve()))
                result = subprocess.run(["npm", "run", "--silent", "anvil:e3-smoke"],
                                        cwd=args.contracts, env=env, text=True, capture_output=True, check=True)
                receipt = json.loads(result.stdout)
                validate_receipt(receipt)
                (args.raw_dir / f"{prover}-{events}-rep{repetition}-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
                samples.append({"events": events, "prover": prover, "repetition": repetition,
                                "leaf_ms": sum(fixture["leafCompressedProofMs"]) / 2,
                                "aggregate_ms": fixture["aggregateGroth16ProofMs"], "receipt": receipt,
                                "fixture_sha256": hashlib.sha256(fixture_path.read_bytes()).hexdigest(),
                                **{key: fixture.get(key) for key in ("workloadDigest", "leafVKeyDigest", "aggregateVKeyDigest", "oldNullifierRoot", "newNullifierRoot", "certificateRoot", "polygonCommitment")}})
                receipts[f"{prover}-{events}-rep{repetition}"] = receipt["transactionHash"]
        for prover in ("cpu", "cuda"):
            group = [row for row in samples if row["events"] == events and row["prover"] == prover]
            label = "gpu" if prover == "cuda" else "cpu"
            import statistics
            timings[f"leaf{events}_{label}_ms"] = statistics.median(row["leaf_ms"] for row in group)
            timings[f"aggregate2{'_leaf64' if events == 64 else ''}_{label}_ms"] = statistics.median(row["aggregate_ms"] for row in group)
    timings.update({"experiment": "E4 preflight A/B", "protocolVersion": 2, "verificationKind": "sp1-groth16",
                    "registry_manifest_sha256": manifest_sha, "acceptedPolicies": policies,
                    "hardware": hardware_metadata(), "samples": samples, "repetitions": 5, "gitCommit": args.git_commit,
                    "topology": "Fabric 2 org / 2 peer / 3 Raft, Gateway, Anvil",
                    "provers": "CPU and CUDA on same compute node", "receipts": receipts,
                    "computeHost": socket.gethostname(), "fabricGateway": args.fabric_gateway,
                    "anvilRpc": args.anvil_rpc, "topologyEvidence": topology_evidence,
                    "sp1SdkVersion": sp1_version})
    validate_preflight(timings)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(timings, indent=2) + "\n")


if __name__ == "__main__":
    main()
