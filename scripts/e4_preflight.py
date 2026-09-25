#!/usr/bin/env python3
"""Run the E4 CPU/GPU A/B gate on one Fabric + Anvil topology."""
import argparse
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
    topology_evidence = verify_topology(args.fabric_gateway, args.anvil_rpc)
    lock = tomllib.loads((Path(__file__).resolve().parents[1] / "crates/sp1-e2e/Cargo.lock").read_text())
    sp1_version = next(package["version"] for package in lock["package"] if package["name"] == "sp1-sdk")
    args.raw_dir.mkdir(parents=True, exist_ok=True)
    timings = {}
    receipts = {}
    for prover, events in (("cpu", 8), ("cuda", 8), ("cpu", 64), ("cuda", 64)):
        fixture_path = args.raw_dir / f"{prover}-{events}-epoch-proof.json"
        command = [str(args.binary.resolve()), "--fabric-gateway", args.fabric_gateway,
                   "--prover", prover, "--events-per-leaf", str(events),
                   "--epoch-id", str(args.epoch_base + (events == 64)), "--out", str(fixture_path)]
        if prover == "cuda":
            command.append("--reuse-fabric-evidence")
        subprocess.run(command,
                       check=True)
        fixture = json.loads(fixture_path.read_text())
        env = dict(os.environ, E3_FIXTURE=str(fixture_path.resolve()), ANVIL_RPC=args.anvil_rpc)
        result = subprocess.run(["npm", "run", "--silent", "anvil:e3-smoke"],
                                cwd=args.contracts, env=env, text=True, capture_output=True, check=True)
        receipt = json.loads(result.stdout)
        if receipt.get("status") != 1:
            raise RuntimeError("preflight Anvil receipt failed")
        (args.raw_dir / f"{prover}-{events}-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        timings[f"leaf{events}_{'gpu' if prover == 'cuda' else 'cpu'}_ms"] = sum(fixture["leafCompressedProofMs"]) / 2
        if events == 8:
            timings[f"aggregate2_{'gpu' if prover == 'cuda' else 'cpu'}_ms"] = fixture["aggregateGroth16ProofMs"]
        else:
            timings[f"aggregate2_leaf64_{'gpu' if prover == 'cuda' else 'cpu'}_ms"] = fixture["aggregateGroth16ProofMs"]
        receipts[f"{prover}-{events}"] = receipt["transactionHash"]
    timings.update({"experiment": "E4 preflight A/B", "gitCommit": args.git_commit,
                    "topology": "Fabric 2 org / 2 peer / 3 Raft, Gateway, Anvil",
                    "provers": "CPU and CUDA on same compute node", "receipts": receipts,
                    "computeHost": socket.gethostname(), "fabricGateway": args.fabric_gateway,
                    "anvilRpc": args.anvil_rpc, "topologyEvidence": topology_evidence,
                    "sp1SdkVersion": sp1_version})
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(timings, indent=2) + "\n")


if __name__ == "__main__":
    main()
