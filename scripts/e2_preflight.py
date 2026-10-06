#!/usr/bin/env python3
"""Collect read-only E2 hardware/topology provenance; no proofs or ledger writes."""
import argparse
import json
import os
import re
import socket
import subprocess
import tomllib
from pathlib import Path
from urllib.parse import urlparse
from e4_preflight import FABRIC_CONTAINERS, hardware_metadata, verify_topology

REPO=Path(__file__).resolve().parents[1]


def validate_record(record):
    if record.get("protocolVersion")!=2 or record.get("kind")!="e2_preflight" or not re.fullmatch(r"[0-9a-f]{40}",record.get("sourceRevision","")):
        raise ValueError("E2 preflight needs protocol-v2 and exact source revision")
    hardware=record.get("hardware",{})
    if any(not hardware.get(key) for key in ("hostname","cpu_model","cpu_logical","ram_bytes","gpu_model_driver","cuda_toolkit","os")):
        raise ValueError("E2 preflight needs CPU/RAM/GPU/driver/CUDA/OS metadata")
    if record.get("computeHost")!=hardware["hostname"]:
        raise ValueError("E2 preflight compute host differs from measured hardware")
    topology=record.get("topology",{})
    if set(topology.get("fabricContainers",{}))!=set(FABRIC_CONTAINERS) or topology.get("anvilChainId")!="0x7a69" or not topology.get("anvilClientVersion"):
        raise ValueError("E2 requires observed 2 org / 2 peer / 3 Raft Fabric and Anvil chain 31337")
    if any(not item.get("id") or not item.get("image") for item in topology["fabricContainers"].values()):
        raise ValueError("E2 Fabric image/container provenance is missing")
    if any(not record.get("software",{}).get(key) for key in ("sp1_version","rustc","cargo")):
        raise ValueError("E2 requires SP1 and Rust/Cargo toolchain provenance")
    if any(urlparse(record.get(key,"")).hostname not in {"localhost","127.0.0.1","::1"} for key in ("fabricGateway","anvilRpc","anchorServer")):
        raise ValueError("E2 formal topology endpoints must be on the compute host")
    return record


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out",type=Path,required=True)
    parser.add_argument("--source-revision",required=True)
    parser.add_argument("--fabric-gateway",default="http://127.0.0.1:8080")
    parser.add_argument("--anvil-rpc",default="http://127.0.0.1:8545")
    parser.add_argument("--anchor-server",default="http://127.0.0.1:8546")
    args=parser.parse_args()
    if args.out.exists() or args.out.resolve().is_relative_to(REPO):parser.error("--out must be a new file outside Git")
    revision=os.environ.get("SOURCE_REVISION") or subprocess.check_output(["git","rev-parse","HEAD"],cwd=REPO,text=True).strip()
    if revision!=args.source_revision:parser.error("requested source revision differs from allocation source")
    packages=tomllib.loads((REPO/"crates/sp1-e2e/Cargo.lock").read_text())["package"]
    record={"kind":"e2_preflight","protocolVersion":2,"sourceRevision":revision,"computeHost":socket.gethostname(),
        "hardware":hardware_metadata(),"topology":verify_topology(args.fabric_gateway,args.anvil_rpc),
        "fabricGateway":args.fabric_gateway,"anvilRpc":args.anvil_rpc,"anchorServer":args.anchor_server,
        "software":{"sp1_version":next(item["version"] for item in packages if item["name"]=="sp1-sdk"),
            "rustc":subprocess.check_output(["rustc","--version"],text=True).strip(),"cargo":subprocess.check_output(["cargo","--version"],text=True).strip()}}
    try:validate_record(record)
    except ValueError as error:parser.error(str(error))
    args.out.parent.mkdir(parents=True,exist_ok=True)
    with args.out.open("x") as output:output.write(json.dumps(record,indent=2)+"\n")


if __name__=="__main__":main()
