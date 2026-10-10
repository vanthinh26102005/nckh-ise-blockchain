#!/usr/bin/env python3
"""Warm both Fabric endorsers with one excluded, read-back EPCIS event."""
import argparse
import base64
import hashlib
import json
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

REPO = Path(__file__).resolve().parents[1]
EVENT_ID = 2**63 - 1


def readback(base, expected, digest):
    try:
        with urlopen(f"{base}/events/{EVENT_ID}", timeout=20) as response:
            row = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return False
        raise
    value = base64.b64decode(row["canonicalEvent"], validate=True)
    if row["eventId"] != str(EVENT_ID) or value != expected or row["digest"] != digest:
        raise ValueError("Fabric warmup readback differs from submitted canonical event")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fabric-gateway", default="http://127.0.0.1:8080")
    args = parser.parse_args()
    if args.out.exists() or not args.out.is_absolute() or args.out.resolve().is_relative_to(REPO):
        parser.error("warmup evidence must be a new absolute path outside Git")
    event = bytearray(86)
    event[0] = 1
    event[1:9] = EVENT_ID.to_bytes(8, "big")
    digest = hashlib.sha256(event).hexdigest()
    body = json.dumps({"canonicalEvent": base64.b64encode(event).decode(), "digest": digest}).encode()
    attempts = []
    for _ in range(3):
        if readback(args.fabric_gateway, event, digest):
            break
        start = time.monotonic()
        try:
            with urlopen(Request(args.fabric_gateway + "/events", body,
                                 {"Content-Type": "application/json"}), timeout=45) as response:
                status = response.status
        except HTTPError as error:
            status = error.code
            error.read()
        except URLError:
            status = "transport-error"
        attempts.append({"httpStatus": status, "elapsedSeconds": time.monotonic() - start})
    if not readback(args.fabric_gateway, event, digest):
        raise RuntimeError("Fabric chaincode did not commit and read back the warmup event")
    record = {"kind": "e3_excluded_chaincode_warmup", "eventId": str(EVENT_ID),
              "sha256": digest, "readbackVerified": True, "excludedFromMeasuredWorkload": True,
              "attempts": attempts}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        json.dump(record, output, indent=2)
        output.write("\n")
    print(f"Fabric warmup committed and read back after {len(attempts)} attempt(s)")


if __name__ == "__main__":
    main()
