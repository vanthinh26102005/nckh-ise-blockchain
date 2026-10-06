#!/usr/bin/env python3
"""Independently check the published E1 local CPU table against its raw CSV."""

import argparse
import csv
import hashlib
import json
import math
import re
from collections import defaultdict
from pathlib import Path


CIRCUITS = {
    "C1": "c1_polygon_outside",
    "C2": "c2_poseidon2_merkle_depth16",
    "C3": "c3_threshold_time",
    "C4": "c4_poseidon2_actor_authorization",
    "C5": "c5_poseidon2_sparse_nullifier_depth32",
}
EVENTS = (8, 16, 32, 64)
METRICS = ("prove_time_s", "verify_time_ms", "proof_size_bytes", "peak_ram_gb")
HEADERS = ("circuit", "events_per_lot", "seed", *METRICS)


def percentile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def read_raw(path):
    cells = defaultdict(dict)
    with path.open(newline="") as stream:
        reader = csv.DictReader(stream)
        if tuple(reader.fieldnames or ()) != HEADERS:
            raise ValueError(f"Unexpected raw CSV columns: {reader.fieldnames}")
        for number, row in enumerate(reader, start=2):
            circuit = row["circuit"]
            events = int(row["events_per_lot"])
            seed = int(row["seed"])
            if circuit not in CIRCUITS.values() or events not in EVENTS or seed not in range(1, 31):
                raise ValueError(f"Unexpected cell or seed at CSV line {number}")
            key = (circuit, events)
            if seed in cells[key]:
                raise ValueError(f"Duplicate cell/seed at CSV line {number}")
            metrics = {name: float(row[name]) for name in METRICS}
            if any(not math.isfinite(value) or value <= 0 for value in metrics.values()):
                raise ValueError(f"Invalid metric at CSV line {number}")
            if not metrics["proof_size_bytes"].is_integer():
                raise ValueError(f"Non-integer proof size at CSV line {number}")
            cells[key][seed] = metrics
    expected = {(name, events) for name in CIRCUITS.values() for events in EVENTS}
    if set(cells) != expected or any(set(seeds) != set(range(1, 31)) for seeds in cells.values()):
        raise ValueError("Incomplete 5 x 4 x 30 grid")
    return cells


def check_report(cells, report_path):
    text = report_path.read_text()
    rows = {}
    for line in text.splitlines():
        if not re.match(r"^\| C[1-5] \|", line):
            continue
        fields = [part.strip() for part in line.strip("|").split("|")]
        key = (CIRCUITS[fields[0]], int(fields[1]))
        if key in rows:
            raise ValueError(f"Duplicate report row: {key}")
        rows[key] = fields
    if set(rows) != set(cells):
        raise ValueError("Report does not contain exactly the 20 expected rows")
    for key, fields in rows.items():
        observations = cells[key].values()
        for position, metric, places in ((2, "prove_time_s", 6), (3, "verify_time_ms", 3)):
            values = [row[metric] for row in observations]
            displayed = [float(value) for value in re.findall(r"\d+\.\d+", fields[position])]
            expected = [percentile(values, fraction) for fraction in (0.5, 0.25, 0.75)]
            if len(displayed) != 3 or any(abs(a - b) >= 0.5 * 10 ** (-places) + 1e-12 for a, b in zip(displayed, expected)):
                raise ValueError(f"Report median/IQR mismatch: {key} {metric}")
        proof = float(fields[4].replace(",", ""))
        expected_proof = percentile([row["proof_size_bytes"] for row in observations], 0.5)
        if proof != expected_proof:
            raise ValueError(f"Report proof median mismatch: {key}")
    return len(rows)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    digest = hashlib.sha256(args.raw.read_bytes()).hexdigest()
    if digest != args.expected_sha256:
        raise SystemExit(f"Raw SHA-256 mismatch: {digest}")
    cells = read_raw(args.raw)
    checked_rows = check_report(cells, args.report)
    maximum = max(row["peak_ram_gb"] for seeds in cells.values() for row in seeds.values())
    result = {
        "raw_sha256": digest,
        "records": sum(len(seeds) for seeds in cells.values()),
        "cells": len(cells),
        "seeds_per_cell": 30,
        "report_rows_checked": checked_rows,
        "max_process_lifetime_peak_rss_gib": maximum,
        "status": "pass",
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
