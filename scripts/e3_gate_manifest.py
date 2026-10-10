#!/usr/bin/env python3
"""Keep E3 gate metadata while leaving raw proof bytes outside Git."""
import argparse
import hashlib
import json
import re
from pathlib import Path


def validate_gate(fixture, receipt, git_commit):
    for label, record in (("fixture", fixture), ("receipt", receipt)):
        if record.get("protocolVersion") != 2 or record.get("verificationKind") != "sp1-groth16":
            raise ValueError(f"{label} requires policy-v2 real SP1 Groth16 verification; stubs cannot pass a formal gate")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", git_commit) or fixture.get("sourceRevision", "").lower() != git_commit.lower():
        raise ValueError("fixture sourceRevision must match the requested 40-character Git commit")
    if receipt.get("sourceRevision", "").lower() != git_commit.lower():
        raise ValueError("receipt sourceRevision must match the proved fixture revision")
    if receipt.get("tamperedAggregateProofRejected") is not True or receipt.get("tamperedAggregateValuesRejected") is not True:
        raise ValueError("aggregate gate requires tampered-proof and tampered-public-values rejection checks")
    public_values = fixture.get("publicValues", "")
    if not re.fullmatch(r"0x[0-9a-fA-F]{704}", public_values):
        raise ValueError("aggregate publicValues must be exactly 352 bytes")
    words = [public_values[2 + index * 64:2 + (index + 1) * 64].lower() for index in range(11)]
    for index in (0, 4, 5):
        if int(words[index], 16) >= 2**64:
            raise ValueError("aggregate ABI uint64 word is not canonical")
    epoch, lots, events = (int(words[index], 16) for index in (0, 4, 5))
    if int(words[6], 16) != 1 or lots != 2 or events != 16:
        raise ValueError("E3 gate requires a valid aggregate of two eight-event lots")
    if words[1] == words[2]:
        raise ValueError("proved nullifier root must advance")
    proof = fixture.get("proof", "")
    if not re.fullmatch(r"0x4388a21c[0-9a-fA-F]{704}", proof):
        raise ValueError("fixture must contain a full proof for the pinned SP1 Groth16 verifier")
    expected = {"epochId": str(epoch), "oldNullifierRoot": "0x" + words[1], "newNullifierRoot": "0x" + words[2],
                "leafVKeyDigest": "0x" + words[7], "aggregateVKeyDigest": "0x" + words[8],
                "certificateRoot": "0x" + words[9], "polygonCommitment": "0x" + words[10]}
    for label, record in (("fixture", fixture), ("receipt", receipt)):
        for field, value in expected.items():
            if str(record.get(field, "")).lower() != value:
                raise ValueError(f"{label} {field} disagrees with proved public values")
    if receipt.get("status") != 1 or receipt.get("publicValuesBytes") != 352 or receipt.get("policyValid") is not True:
        raise ValueError("Anvil receipt must record successful policy-v2 verification")
    for field, expected_value in (("lotCount", lots), ("eventCount", events)):
        if str(receipt.get(field)) != str(expected_value):
            raise ValueError(f"receipt {field} disagrees with public values")
    if not re.fullmatch(r"0x[0-9a-fA-F]{64}", receipt.get("transactionHash", "")):
        raise ValueError("Anvil receipt has no transaction hash")
    if not re.fullmatch(r"0x[0-9a-fA-F]{64}", fixture.get("vkey", "")):
        raise ValueError("fixture has no aggregate program verification key")
    for field in ("anchor", "registry"):
        if not re.fullmatch(r"0x[0-9a-fA-F]{40}", receipt.get(field, "")):
            raise ValueError(f"receipt has no {field} address")
    if receipt.get("programVKey", "").lower() != fixture["vkey"].lower():
        raise ValueError("receipt program key does not match the fixture")
    event = receipt.get("anchorEvent", {})
    if event.get("name") != "EpochAggregated" or event.get("address", "").lower() != receipt["anchor"].lower():
        raise ValueError("receipt is missing its anchor's EpochAggregated event")
    if event.get("transactionHash", "").lower() != receipt["transactionHash"].lower() or not re.fullmatch(r"0x[0-9a-fA-F]{64}", event.get("blockHash", "")):
        raise ValueError("aggregate event is not bound to the mined transaction")
    expected_event = {"epochId": str(epoch), "epochCommitment": "0x" + words[3], "oldNullifierRoot": expected["oldNullifierRoot"],
                      "newNullifierRoot": expected["newNullifierRoot"], "lotCount": str(lots), "eventCount": str(events)}
    for field, value in expected_event.items():
        if str(event.get("args", {}).get(field, "")).lower() != value:
            raise ValueError(f"aggregate event {field} disagrees with the proved transition")
    if (fixture.get("fabricEventsAcknowledged") != 16 or fixture.get("fabricEventsReadback") != 16
            or fixture.get("fabricEvidenceReused") is not False):
        raise ValueError("E3 gate requires two newly committed and exactly read-back eight-event Fabric lots")
    if fixture.get("tamperedChildRejected") is not True:
        raise ValueError("E3 gate requires an SP1 tampered-child rejection check")


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
    try:
        validate_gate(fixture, receipt, args.git_commit)
    except (ValueError, TypeError, KeyError) as error:
        parser.error(str(error))
    manifest = {
        "experiment": "E3 aggregate gate",
        "protocolVersion": 2,
        "verificationKind": "sp1-groth16",
        "tamperedAggregateProofRejected": True,
        "tamperedAggregateValuesRejected": True,
        "certificateRoot": receipt["certificateRoot"],
        "polygonCommitment": receipt["polygonCommitment"],
        "publicValuesSha256": hashlib.sha256(bytes.fromhex(fixture["publicValues"][2:])).hexdigest(),
        "proofSha256": hashlib.sha256(bytes.fromhex(fixture["proof"][2:])).hexdigest(),
        "receiptSha256": hashlib.sha256(args.receipt.read_bytes()).hexdigest(),
        "anchor": receipt["anchor"],
        "registry": receipt["registry"],
        "anchorEvent": receipt["anchorEvent"],
        "status": "passed",
        "gitCommit": args.git_commit,
        "fixtureSha256": hashlib.sha256(fixture_bytes).hexdigest(),
        "epochId": fixture["epochId"],
        "policyProgramVKeyDigest": fixture["leafVKeyDigest"],
        "aggregateProgramVKeyDigest": fixture["aggregateVKeyDigest"],
        "aggregateProgramVKey": fixture["vkey"],
        "fabricEventsAcknowledged": 16,
        "fabricEventsReadback": 16,
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
    try:
        with args.out.open("x") as output:
            output.write(json.dumps(manifest, indent=2) + "\n")
    except FileExistsError:
        parser.error("output manifest already exists; choose a new output path")


if __name__ == "__main__":
    main()
