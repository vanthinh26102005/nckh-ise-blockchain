#!/usr/bin/env python3
"""Generate the deterministic E4 sweep plan (screening + confirmation).

E4 sweeps ONLY the proposed system. Each cell runs exactly 4 complete epochs.
Screening: Np{125,250,500,1000,2000} x shipment{16,32,64,128} x epoch{30,60,120,300}, 1 seed/cell.
Confirmation: Np{125,500,2000} x shipment{16,64,128} x epoch{30,120,300}, 10 seeds/cell.

The plan is committed (it is configuration, not measurement) and is consumed by
scripts/e4_run.py. Seeds are derived deterministically from the cell id so the plan
is stable across regenerations. Stdlib only — no third-party deps.
"""

import argparse
import csv
import hashlib
import json
from pathlib import Path

# Per-producer Poisson expectation (events/minute), per issue #22.
LAMBDA_EVENTS_PER_MIN = 3.84
EPOCHS_TARGET = 4

PHASES = {
    "screening": {
        "np": [125, 250, 500, 1000, 2000],
        "shipment": [16, 32, 64, 128],
        "epoch_s": [30, 60, 120, 300],
        "seeds_per_cell": 1,
    },
    "confirmation": {
        "np": [125, 500, 2000],
        "shipment": [16, 64, 128],
        "epoch_s": [30, 120, 300],
        "seeds_per_cell": 10,
    },
}

FIELDS = [
    "phase",
    "cell_id",
    "run_id",
    "np",
    "shipment",
    "epoch_s",
    "seed",
    "seed_index",
    "seeds_per_cell",
    "lambda_events_per_min",
    "offered_load_epm",
    "epochs_target",
]


def cell_id(phase, np_, shipment, epoch_s):
    return f"{phase}-np{np_}-sh{shipment}-ep{epoch_s}"


def derive_seed(cid, seed_index):
    """Deterministic non-negative 32-bit seed from cell id + index."""
    h = hashlib.sha256(f"{cid}#s{seed_index}".encode()).digest()
    return int.from_bytes(h[:4], "big")


def generate_rows(phases):
    for phase, cfg in phases.items():
        spc = cfg["seeds_per_cell"]
        for np_ in cfg["np"]:
            for shipment in cfg["shipment"]:
                for epoch_s in cfg["epoch_s"]:
                    cid = cell_id(phase, np_, shipment, epoch_s)
                    for seed_index in range(spc):
                        seed = derive_seed(cid, seed_index)
                        yield {
                            "phase": phase,
                            "cell_id": cid,
                            "run_id": f"{cid}#s{seed}",
                            "np": np_,
                            "shipment": shipment,
                            "epoch_s": epoch_s,
                            "seed": seed,
                            "seed_index": seed_index,
                            "seeds_per_cell": spc,
                            "lambda_events_per_min": LAMBDA_EVENTS_PER_MIN,
                            "offered_load_epm": round(LAMBDA_EVENTS_PER_MIN * np_, 4),
                            "epochs_target": EPOCHS_TARGET,
                        }


def main():
    parser = argparse.ArgumentParser(description="E4 sweep plan generator")
    parser.add_argument(
        "--phase",
        choices=["screening", "confirmation", "all"],
        default="all",
        help="Which phase(s) to emit.",
    )
    parser.add_argument("--out", type=Path, default=Path("results/e4/plan.csv"))
    parser.add_argument(
        "--summary-out",
        type=Path,
        default=None,
        help="Optional JSON summary (counts per phase).",
    )
    args = parser.parse_args()

    phases = PHASES if args.phase == "all" else {args.phase: PHASES[args.phase]}
    rows = list(generate_rows(phases))

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)

    summary = {"phases": {}, "runs_total": len(rows)}
    for phase, cfg in phases.items():
        cells = len(cfg["np"]) * len(cfg["shipment"]) * len(cfg["epoch_s"])
        summary["phases"][phase] = {
            "cells": cells,
            "seeds_per_cell": cfg["seeds_per_cell"],
            "runs": cells * cfg["seeds_per_cell"],
        }

    if args.summary_out:
        args.summary_out.parent.mkdir(parents=True, exist_ok=True)
        args.summary_out.write_text(json.dumps(summary, indent=2) + "\n")

    print(f"Wrote plan: {args.out} ({len(rows)} runs)")
    for phase, s in summary["phases"].items():
        print(f"  {phase}: {s['cells']} cells x {s['seeds_per_cell']} seed = {s['runs']} runs")


if __name__ == "__main__":
    main()
