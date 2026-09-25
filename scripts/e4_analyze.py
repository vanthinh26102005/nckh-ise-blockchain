#!/usr/bin/env python3
"""Analyze an E4 sweep run: validate, aggregate across seeds, emit tables + figures.

Consumes the raw JSONL from scripts/e4_run.py and its committed manifest. Produces:
  * summary.csv          -- per-cell aggregate (throughput, completion, proving, gas, calldata)
  * policy_tradeoff.csv  -- shipment x epoch trade-off aggregated over producers
  * report.md            -- human-readable report with censored/saturated transparency
  * heatmap_throughput.png / heatmap_completion.png  (faceted by epoch; needs matplotlib)
  * proving_time.png     -- leaf + aggregate proving time vs shipment (needs matplotlib)

Censored/saturated cells are NEVER dropped: they are kept in every table and reported
explicitly. Plots hatch saturated/censored cells instead of hiding them.

Guardrails (issue #22):
  --require-real          reject mock-executor data (mock is a harness self-test only)
  --require-single-host   reject split-host runs (not official E4)
Seeds-per-cell (1 screening / 10 confirmation) and the exactly-4-epochs rule are checked.
"""

import argparse
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path

EPOCHS_TARGET = 4


def read_jsonl(path):
    records = []
    for line in Path(path).read_text().splitlines():
        line = line.strip()
        if line:
            records.append(json.loads(line))
    return [r for r in records if r.get("kind") == "cell"]


def mean_or_none(values):
    values = [v for v in values if v is not None]
    return statistics.fmean(values) if values else None


def display(value, places=1):
    return f"{value:.{places}f}" if value is not None else "n/a"


def aggregate_cells(records):
    """Group runs by cell_id and aggregate across seeds."""
    by_cell = defaultdict(list)
    for r in records:
        by_cell[r["cell_id"]].append(r)

    cells = []
    for cell_id, runs in sorted(by_cell.items()):
        first = runs[0]
        statuses = [r["status"] for r in runs]
        ok_runs = [r for r in runs if r["status"] == "ok"]
        # Aggregate metrics over ALL runs (censored kept), plus an ok-only view.
        cells.append({
            "cell_id": cell_id,
            "phase": first["phase"],
            "np": first["np"],
            "shipment": first["shipment"],
            "epoch_s": first["epoch_s"],
            "runs": len(runs),
            "seeds": sorted({r["seed"] for r in runs}),
            "n_ok": statuses.count("ok"),
            "n_saturated": statuses.count("saturated"),
            "n_censored": statuses.count("censored"),
            "throughput_eps_mean": mean_or_none([r["throughput_eps"] for r in runs]),
            "throughput_eps_ok_mean": mean_or_none([r["throughput_eps"] for r in ok_runs]),
            "completion_rate_mean": mean_or_none([r["completion_rate"] for r in runs]),
            "leaf_p50_ms": mean_or_none([r["leaf_proving_ms"]["p50"] for r in runs]),
            "leaf_p95_ms": mean_or_none([r["leaf_proving_ms"]["p95"] for r in runs]),
            "aggregate_p50_ms": mean_or_none([r["aggregate_proving_ms"]["p50"] for r in runs]),
            "aggregate_p95_ms": mean_or_none([r["aggregate_proving_ms"]["p95"] for r in runs]),
            "audit_p50_ms": mean_or_none([r["audit_latency_ms"]["p50"] for r in runs]),
            "audit_p95_ms": mean_or_none([r["audit_latency_ms"]["p95"] for r in runs]),
            "gas_mean": mean_or_none([r["gas_used"]["mean"] for r in runs]),
            "calldata_bytes_mean": mean_or_none([r["calldata_bytes"]["mean"] for r in runs]),
        })
    return cells


def validate(records, manifest, cells, require_real, require_single_host, plan_rows=None):
    errors = []
    warnings = []

    executors = {r["executor"] for r in records}
    if require_real and executors != {"real-e3"}:
        errors.append(f"--require-real set but executor(s) = {sorted(executors)}; mock is not E4 evidence.")
    if require_real and (manifest.get("e3_gate_passed") is not True or not manifest.get("preflight_ab")):
        errors.append("real E4 requires a passed E3 gate and measured CPU/GPU preflight")
    if "mock" in executors:
        warnings.append("Data contains mock-executor runs (harness self-test, NOT E4 evidence).")

    if any(r.get("split_host") for r in records):
        msg = "Run contains split-host records (GPU proving off-host from Fabric/Anvil)."
        (errors if require_single_host else warnings).append(msg)

    expected_spc = manifest.get("seeds_per_cell")
    run_ids = [r["run_id"] for r in records]
    if len(run_ids) != len(set(run_ids)):
        errors.append("duplicate run_id in raw E4 records")
    if manifest.get("runs_total") != len(records):
        errors.append(f"manifest runs_total={manifest.get('runs_total')} but raw has {len(records)} records")
    if plan_rows is not None:
        planned = {row["run_id"] for row in plan_rows if row["phase"] == manifest.get("phase")}
        actual = set(run_ids)
        if actual != planned:
            errors.append(f"raw E4 run IDs differ from committed plan: missing={len(planned - actual)}, extra={len(actual - planned)}")
    for c in cells:
        if expected_spc and c["runs"] != expected_spc:
            errors.append(f"Cell {c['cell_id']}: {c['runs']} runs, expected {expected_spc} seeds/cell.")
        if len(c["seeds"]) != c["runs"]:
            errors.append(f"Cell {c['cell_id']} repeats a seed")

    # Exactly-4-epochs rule for cells claiming ok.
    for r in records:
        if r["status"] == "ok" and r.get("epochs_completed") != EPOCHS_TARGET:
            errors.append(f"Run {r['run_id']}: status=ok but epochs_completed={r.get('epochs_completed')} != {EPOCHS_TARGET}.")
        if r["status"] == "ok" and (not r.get("queue_drained") or r.get("committed_events") != r.get("offered_events")):
            errors.append(f"Run {r['run_id']}: status=ok without a drained, fully committed queue")
        if require_real and r["status"] == "ok" and (not r.get("offered_shipments")
                or r.get("committed_shipments") != r.get("offered_shipments")):
            errors.append(f"Run {r['run_id']}: status=ok without all shipments committed")
        if require_real and len(r.get("transactions", [])) != r.get("epochs_completed"):
            errors.append(f"Run {r['run_id']}: missing Anvil receipt for a completed epoch")

    return errors, warnings


def write_summary_csv(cells, path):
    fields = [
        "cell_id", "phase", "np", "shipment", "epoch_s", "runs",
        "n_ok", "n_saturated", "n_censored",
        "throughput_eps_mean", "throughput_eps_ok_mean", "completion_rate_mean",
        "leaf_p50_ms", "leaf_p95_ms", "aggregate_p50_ms", "aggregate_p95_ms",
        "audit_p50_ms", "audit_p95_ms", "gas_mean", "calldata_bytes_mean",
    ]
    with Path(path).open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for c in cells:
            w.writerow({k: c[k] for k in fields})


def write_policy_tradeoff_csv(cells, path):
    """Aggregate over producers: shipment x epoch trade-off."""
    by_key = defaultdict(list)
    for c in cells:
        by_key[(c["shipment"], c["epoch_s"])].append(c)
    fields = [
        "shipment", "epoch_s", "cells",
        "throughput_eps_mean", "completion_rate_mean",
        "leaf_p50_ms", "aggregate_p50_ms", "audit_p50_ms",
        "gas_mean", "calldata_bytes_mean",
        "n_saturated", "n_censored",
    ]
    rows = []
    for (shipment, epoch_s), group in sorted(by_key.items()):
        rows.append({
            "shipment": shipment,
            "epoch_s": epoch_s,
            "cells": len(group),
            "throughput_eps_mean": mean_or_none([g["throughput_eps_mean"] for g in group]),
            "completion_rate_mean": mean_or_none([g["completion_rate_mean"] for g in group]),
            "leaf_p50_ms": mean_or_none([g["leaf_p50_ms"] for g in group]),
            "aggregate_p50_ms": mean_or_none([g["aggregate_p50_ms"] for g in group]),
            "audit_p50_ms": mean_or_none([g["audit_p50_ms"] for g in group]),
            "gas_mean": mean_or_none([g["gas_mean"] for g in group]),
            "calldata_bytes_mean": mean_or_none([g["calldata_bytes_mean"] for g in group]),
            "n_saturated": sum(g["n_saturated"] for g in group),
            "n_censored": sum(g["n_censored"] for g in group),
        })
    with Path(path).open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)
    return rows


def write_heatmaps(cells, out_dir, metric, title, fname):
    """Heatmap of `metric` over np (rows) x shipment (cols), one subplot per epoch.

    Saturated/censored cells are hatched. Requires matplotlib; skipped if unavailable.
    """
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        import numpy as np
    except ImportError:
        print(f"matplotlib/numpy unavailable; skipping figure {fname}.")
        return

    nps = sorted({c["np"] for c in cells})
    ships = sorted({c["shipment"] for c in cells})
    epochs = sorted({c["epoch_s"] for c in cells})
    index = {(c["np"], c["shipment"], c["epoch_s"]): c for c in cells}

    fig, axes = plt.subplots(1, len(epochs), figsize=(5 * len(epochs), 4.5), squeeze=False)
    for ax, epoch_s in zip(axes[0], epochs):
        grid = np.full((len(nps), len(ships)), np.nan)
        for i, np_ in enumerate(nps):
            for j, sh in enumerate(ships):
                c = index.get((np_, sh, epoch_s))
                if c and c[metric] is not None:
                    grid[i, j] = c[metric]
        im = ax.imshow(grid, aspect="auto", origin="lower", cmap="viridis")
        ax.set_xticks(range(len(ships)))
        ax.set_xticklabels(ships)
        ax.set_yticks(range(len(nps)))
        ax.set_yticklabels(nps)
        ax.set_xlabel("shipment (events/lot)")
        ax.set_ylabel("Np (producers)")
        ax.set_title(f"epoch = {epoch_s}s")
        # Hatch saturated/censored cells for transparency.
        for i, np_ in enumerate(nps):
            for j, sh in enumerate(ships):
                c = index.get((np_, sh, epoch_s))
                if c and (c["n_saturated"] or c["n_censored"]):
                    ax.add_patch(plt.Rectangle((j - 0.5, i - 0.5), 1, 1, fill=False,
                                               hatch="///", edgecolor="red", linewidth=0))
                if c and c[metric] is not None:
                    ax.text(j, i, display(c[metric], 0 if c[metric] >= 10 else 2),
                            ha="center", va="center", color="white", fontsize=7)
        fig.colorbar(im, ax=ax, fraction=0.046, pad=0.04)
    fig.suptitle(f"{title}  (red hatch = saturated/censored)")
    fig.tight_layout()
    out = Path(out_dir) / fname
    fig.savefig(out, dpi=150)
    plt.close(fig)
    print(f"Saved figure: {out}")


def write_proving_plot(policy_rows, out_dir, fname="proving_time.png"):
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError:
        print(f"matplotlib unavailable; skipping figure {fname}.")
        return
    by_ship = defaultdict(lambda: {"leaf": [], "agg": []})
    for r in policy_rows:
        by_ship[r["shipment"]]["leaf"].append(r["leaf_p50_ms"])
        by_ship[r["shipment"]]["agg"].append(r["aggregate_p50_ms"])
    ships = sorted(by_ship)
    leaf = [mean_or_none(by_ship[s]["leaf"]) for s in ships]
    agg = [mean_or_none(by_ship[s]["agg"]) for s in ships]
    fig, ax = plt.subplots(figsize=(7, 4.5))
    ax.plot(ships, leaf, marker="o", label="leaf proving p50")
    ax.plot(ships, agg, marker="s", label="aggregate proving p50")
    ax.set_xlabel("shipment (events/lot)")
    ax.set_ylabel("proving time (ms)")
    ax.set_title("E4 proving time vs shipment size")
    ax.grid(True, alpha=0.3)
    ax.legend()
    fig.tight_layout()
    out = Path(out_dir) / fname
    fig.savefig(out, dpi=150)
    plt.close(fig)
    print(f"Saved figure: {out}")


def write_report(cells, records, manifest, errors, warnings, policy_rows, path):
    total_runs = len(records)
    n_ok = sum(1 for r in records if r["status"] == "ok")
    n_sat = sum(1 for r in records if r["status"] == "saturated")
    n_cen = sum(1 for r in records if r["status"] == "censored")
    censored_cells = [c for c in cells if c["n_saturated"] or c["n_censored"]]

    lines = [
        f"# E4 Scalability Report — {manifest.get('phase', '?')} phase",
        "",
        f"- **Executor**: `{manifest.get('executor')}`"
        + ("  ⚠️ mock = harness self-test, NOT E4 evidence" if manifest.get("executor") == "mock" else ""),
        f"- **Grid**: Np={manifest.get('grid', {}).get('np')} × "
        f"shipment={manifest.get('grid', {}).get('shipment')} × "
        f"epoch_s={manifest.get('grid', {}).get('epoch_s')}",
        f"- **Seeds/cell**: {manifest.get('seeds_per_cell')} · **Epochs/run**: {EPOCHS_TARGET}",
        f"- **Runs**: {total_runs}  (ok={n_ok}, saturated={n_sat}, censored={n_cen})",
        f"- **E3 gate passed**: {manifest.get('e3_gate_passed')}",
        "",
    ]

    if errors:
        lines += ["## ❌ Validation errors", ""] + [f"- {e}" for e in errors] + [""]
    if warnings:
        lines += ["## ⚠️ Warnings", ""] + [f"- {w}" for w in warnings] + [""]

    lines += [
        "## Censored / saturated cells (kept, not dropped)",
        "",
    ]
    if censored_cells:
        lines += ["| cell | Np | shipment | epoch_s | saturated | censored |",
                  "|---|---:|---:|---:|---:|---:|"]
        for c in censored_cells:
            lines.append(f"| `{c['cell_id']}` | {c['np']} | {c['shipment']} | {c['epoch_s']} "
                         f"| {c['n_saturated']} | {c['n_censored']} |")
    else:
        lines.append("_None — every cell completed 4 clean epochs and drained._")
    lines.append("")

    lines += [
        "## Policy trade-off (aggregated over producers)",
        "",
        "| shipment | epoch_s | throughput (eps) | completion | leaf p50 (ms) | aggregate p50 (ms) | gas | calldata (B) | saturated | censored |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for r in policy_rows:
        lines.append(
            f"| {r['shipment']} | {r['epoch_s']} | {display(r['throughput_eps_mean'], 2)} | "
            f"{display(r['completion_rate_mean'], 3)} | {display(r['leaf_p50_ms'])} | {display(r['aggregate_p50_ms'])} | "
            f"{display(r['gas_mean'], 0)} | {display(r['calldata_bytes_mean'], 0)} | {r['n_saturated']} | {r['n_censored']} |"
        )
    lines.append("")
    lines.append("Full per-cell numbers: `summary.csv`. Policy grid: `policy_tradeoff.csv`.")

    Path(path).write_text("\n".join(lines) + "\n")
    print(f"Saved report: {path}")


def main():
    parser = argparse.ArgumentParser(description="E4 scalability analysis")
    parser.add_argument("--raw", type=Path, required=True, help="Raw JSONL from e4_run.py")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--require-real", action="store_true",
                        help="Reject mock-executor data (mock is not E4 evidence).")
    parser.add_argument("--require-single-host", action="store_true",
                        help="Reject split-host runs (not official E4).")
    parser.add_argument("--no-figures", action="store_true")
    parser.add_argument("--allow-partial", action="store_true", help="Pilot only; skip full committed-plan check.")
    args = parser.parse_args()

    args.out_dir.mkdir(parents=True, exist_ok=True)
    records = read_jsonl(args.raw)
    if not records:
        raise SystemExit(f"No cell records in {args.raw}")
    manifest = json.loads(Path(args.manifest).read_text())

    cells = aggregate_cells(records)
    plan_rows = None
    if not args.allow_partial:
        plan_path = Path(manifest.get("plan_path", ""))
        if not plan_path.is_file():
            raise SystemExit(f"Committed E4 plan is missing: {plan_path}")
        with plan_path.open(newline="") as f:
            plan_rows = list(csv.DictReader(f))
    errors, warnings = validate(records, manifest, cells, args.require_real, args.require_single_host, plan_rows)

    write_summary_csv(cells, args.out_dir / "summary.csv")
    policy_rows = write_policy_tradeoff_csv(cells, args.out_dir / "policy_tradeoff.csv")

    if not args.no_figures:
        write_heatmaps(cells, args.out_dir, "throughput_eps_mean",
                       "E4 throughput (committed events/s)", "heatmap_throughput.png")
        write_heatmaps(cells, args.out_dir, "completion_rate_mean",
                       "E4 completion rate", "heatmap_completion.png")
        write_proving_plot(policy_rows, args.out_dir)

    write_report(cells, records, manifest, errors, warnings, policy_rows,
                 args.out_dir / "report.md")

    for w in warnings:
        print(f"WARN: {w}")
    if errors:
        for e in errors:
            print(f"ERROR: {e}")
        raise SystemExit(f"E4 validation failed with {len(errors)} error(s).")
    print("E4 analysis complete.")


if __name__ == "__main__":
    main()
