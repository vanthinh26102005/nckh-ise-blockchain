#!/usr/bin/env python3
"""Keep E3 gate metadata while leaving raw proof bytes outside Git."""
import argparse
import hashlib
import json
import re
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--git-commit", required=True)
    args = parser.parse_args()
    fixture_bytes = args.fixture.read_bytes()
    fixture = json.loads(fixture_bytes)
    receipt = json.loads(args.receipt.read_text())
    if receipt.get("status") != 1 or receipt.get("newNullifierRoot", "").lower() != fixture["newNullifierRoot"].lower():
        parser.error("Anvil receipt must be successful and match the proved root")
    if not re.fullmatch(r"0x[0-9a-fA-F]{64}", receipt.get("transactionHash", "")):
        parser.error("Anvil receipt has no transaction hash")
    if fixture.get("fabricEventsAcknowledged") != 16:
        parser.error("E3 gate requires two eight-event Fabric lots")
    if fixture.get("tamperedChildRejected") is not True:
        parser.error("E3 gate requires an SP1 tampered-child rejection check")
    manifest = {
        "experiment": "E3 aggregate gate",
        "status": "passed",
        "gitCommit": args.git_commit,
        "fixtureSha256": hashlib.sha256(fixture_bytes).hexdigest(),
        "epochId": fixture["epochId"],
        "policyProgramVKeyDigest": fixture["leafVKeyDigest"],
        "aggregateProgramVKeyDigest": fixture["aggregateVKeyDigest"],
        "aggregateProgramVKey": fixture["vkey"],
        "fabricEventsAcknowledged": 16,
        "tamperedChildRejected": True,
        "leafCompressedProofMs": fixture["leafCompressedProofMs"],
        "aggregateGroth16ProofMs": fixture["aggregateGroth16ProofMs"],
        "anvilReceiptStatus": 1,
        "transactionHash": receipt["transactionHash"],
        "gasUsed": receipt["gasUsed"],
        "calldataBytes": receipt["calldataBytes"],
        "newNullifierRoot": receipt["newNullifierRoot"],
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
