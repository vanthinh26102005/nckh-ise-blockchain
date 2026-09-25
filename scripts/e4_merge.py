#!/usr/bin/env python3
"""Assemble immutable single-run E4 shards into one complete phase for analysis."""
import argparse
import csv
import json
from pathlib import Path

from e4_analyze import aggregate_cells, validate

REPO = Path(__file__).resolve().parents[1]


def merge(plan_rows, shards, phase):
    expected = {row["run_id"]: row for row in plan_rows if row["phase"] == phase}
    if not expected:
        raise ValueError("phase is absent from plan")
    records = {}
    template = None
    for raw in shards:
        manifest_path = raw.with_name("manifest.json")
        if not manifest_path.is_file():
            raise ValueError(f"missing shard manifest: {manifest_path}")
        manifest = json.loads(manifest_path.read_text())
        lines = [json.loads(line) for line in raw.read_text().splitlines() if line.strip()]
        if len(lines) != 1 or manifest.get("runs_total") != 1:
            raise ValueError(f"E4 shard must contain exactly one run: {raw}")
        record = lines[0]
        run_id = record["run_id"]
        if run_id not in expected or run_id in records or record.get("phase") != phase:
            raise ValueError(f"unknown, duplicate or wrong-phase E4 run: {run_id}")
        row = expected[run_id]
        if any(str(record[key]) != row[key] for key in ("np", "shipment", "epoch_s", "seed")):
            raise ValueError(f"E4 shard does not match plan axes: {run_id}")
        fixed = ("executor", "phase", "e3_gate_passed", "e3_manifest_ref", "preflight_ab", "software", "topology")
        if template and any(manifest.get(key) != template.get(key) for key in fixed):
            raise ValueError(f"E4 shard manifest differs from first shard: {raw}")
        if manifest.get("executor") != "real-e3":
            raise ValueError("mock shards cannot become E4 evidence")
        template = template or manifest
        records[run_id] = record
    missing = set(expected) - set(records)
    if missing:
        raise ValueError(f"E4 phase incomplete: {len(missing)} missing run IDs")
    ordered = [records[row["run_id"]] for row in plan_rows if row["phase"] == phase]
    template["runs_total"] = len(ordered)
    template["cells_total"] = len({row["cell_id"] for row in ordered})
    template["grid"] = {key: sorted({row[key] for row in ordered})
                        for key in ("np", "shipment", "epoch_s")}
    template["seeds_per_cell"] = 1 if phase == "screening" else 10
    template["command"] = "scripts/e4_merge.py (complete immutable phase)"
    errors, _ = validate(ordered, template, aggregate_cells(ordered), True, True, plan_rows)
    if errors:
        raise ValueError("E4 merged phase failed validation: " + "; ".join(errors))
    return ordered, template


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", choices=("screening", "confirmation"), required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--shard-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--manifest-out", type=Path, required=True)
    args = parser.parse_args()
    if args.out.resolve().is_relative_to(REPO) or args.out.exists() or args.manifest_out.exists():
        parser.error("merged raw must be a new file outside Git; manifest must be new")
    with args.plan.open(newline="") as file:
        plan_rows = list(csv.DictReader(file))
    shards = sorted(args.shard_root.rglob("raw.jsonl"))
    records, manifest = merge(plan_rows, shards, args.phase)
    manifest["raw_path"] = str(args.out.resolve())
    manifest["plan_path"] = str(args.plan)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.manifest_out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        output.write("".join(json.dumps(record) + "\n" for record in records))
    with args.manifest_out.open("x") as output:
        output.write(json.dumps(manifest, indent=2) + "\n")
    print(f"E4 {args.phase}: merged {len(records)} real runs -> {args.out}")


if __name__ == "__main__":
    main()
