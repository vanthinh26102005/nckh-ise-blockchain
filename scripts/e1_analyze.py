#!/usr/bin/env python3
import argparse
import csv
import math
import os
import statistics
from collections import defaultdict


SUMMARY_METRICS = (
    "prove_ms",
    "verify_ms",
    "proof_bytes",
    "peak_rss_mb",
    "build_ms",
    "witness_ms",
    "setup_ms",
    "gate_count",
    "public_inputs",
    "inner_prove_ms",
)
PLOT_METRICS = ("prove_ms", "verify_ms", "proof_bytes", "peak_rss_mb")


def mean_ci(values):
    if not values:
        return "", "", ""
    mean = statistics.fmean(values)
    if len(values) == 1:
        return mean, mean, mean
    stdev = statistics.stdev(values)
    half_width = 1.96 * stdev / math.sqrt(len(values))
    return mean, mean - half_width, mean + half_width


def read_rows(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f))


def write_summary(rows, out_path):
    groups = defaultdict(list)
    for row in rows:
        if row["status"] == "ok":
            groups[(row["events_per_lot"], row["circuit"])].append(row)

    fields = ["events_per_lot", "circuit", "n"]
    available_metrics = [metric for metric in SUMMARY_METRICS if rows and metric in rows[0]]
    for metric in available_metrics:
        fields.extend([f"{metric}_mean", f"{metric}_ci95_low", f"{metric}_ci95_high"])

    with open(out_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for (events, circuit), group in sorted(groups.items(), key=lambda x: (int(x[0][0]), x[0][1])):
            out = {"events_per_lot": events, "circuit": circuit, "n": len(group)}
            for metric in available_metrics:
                values = [float(r[metric]) for r in group]
                mean, low, high = mean_ci(values)
                out[f"{metric}_mean"] = f"{mean:.6f}" if mean != "" else ""
                out[f"{metric}_ci95_low"] = f"{low:.6f}" if low != "" else ""
                out[f"{metric}_ci95_high"] = f"{high:.6f}" if high != "" else ""
            writer.writerow(out)


def write_svg_plot(rows, metric, out_path):
    groups = defaultdict(list)
    for row in rows:
        if row["status"] == "ok":
            groups[(row["circuit"], int(row["events_per_lot"]))].append(float(row[metric]))

    circuits = sorted({c for c, _ in groups})
    events = sorted({e for _, e in groups})
    if not circuits or not events:
        return

    series = {
        circuit: [(event, statistics.fmean(groups[(circuit, event)])) for event in events if (circuit, event) in groups]
        for circuit in circuits
    }
    all_values = [value for points in series.values() for _, value in points]
    max_value = max(all_values) if all_values else 1.0
    max_value = max(max_value, 1.0)

    width, height = 960, 560
    left, right, top, bottom = 80, 30, 40, 80
    plot_w = width - left - right
    plot_h = height - top - bottom
    palette = ["#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#8c564b", "#17becf"]

    def x_scale(event):
        if len(events) == 1:
            return left + plot_w / 2
        return left + (event - min(events)) * plot_w / (max(events) - min(events))

    def y_scale(value):
        return top + plot_h - (value / max_value) * plot_h

    title = {
        "prove_ms": "Prove time (ms)",
        "verify_ms": "Verify time (ms)",
        "proof_bytes": "Compressed proof size (bytes)",
        "peak_rss_mb": "Peak RSS (MB)",
    }[metric]

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="white"/>',
        f'<text x="{width/2}" y="24" text-anchor="middle" font-family="Arial" font-size="18">{title}</text>',
        f'<line x1="{left}" y1="{top}" x2="{left}" y2="{top+plot_h}" stroke="#222"/>',
        f'<line x1="{left}" y1="{top+plot_h}" x2="{left+plot_w}" y2="{top+plot_h}" stroke="#222"/>',
    ]

    for event in events:
        x = x_scale(event)
        lines.append(f'<line x1="{x:.2f}" y1="{top+plot_h}" x2="{x:.2f}" y2="{top+plot_h+6}" stroke="#222"/>')
        lines.append(f'<text x="{x:.2f}" y="{top+plot_h+24}" text-anchor="middle" font-family="Arial" font-size="12">{event}</text>')

    for i in range(5):
        value = max_value * i / 4
        y = y_scale(value)
        lines.append(f'<line x1="{left-6}" y1="{y:.2f}" x2="{left}" y2="{y:.2f}" stroke="#222"/>')
        lines.append(f'<text x="{left-10}" y="{y+4:.2f}" text-anchor="end" font-family="Arial" font-size="12">{value:.1f}</text>')
        lines.append(f'<line x1="{left}" y1="{y:.2f}" x2="{left+plot_w}" y2="{y:.2f}" stroke="#eee"/>')

    for idx, circuit in enumerate(circuits):
        color = palette[idx % len(palette)]
        points = series[circuit]
        if not points:
            continue
        polyline = " ".join(f"{x_scale(e):.2f},{y_scale(v):.2f}" for e, v in points)
        lines.append(f'<polyline points="{polyline}" fill="none" stroke="{color}" stroke-width="2"/>')
        for event, value in points:
            lines.append(f'<circle cx="{x_scale(event):.2f}" cy="{y_scale(value):.2f}" r="4" fill="{color}"/>')
        legend_y = top + 18 + idx * 20
        lines.append(f'<rect x="{left+plot_w-140}" y="{legend_y-10}" width="12" height="12" fill="{color}"/>')
        lines.append(f'<text x="{left+plot_w-122}" y="{legend_y}" font-family="Arial" font-size="12">{circuit}</text>')

    lines.append(f'<text x="{left+plot_w/2}" y="{height-24}" text-anchor="middle" font-family="Arial" font-size="14">events/lot</text>')
    lines.append("</svg>")

    with open(out_path, "w") as f:
        f.write("\n".join(lines))


def write_report(rows, out_path):
    ok_rows = [r for r in rows if r["status"] == "ok"]
    error_rows = [r for r in rows if r["status"] != "ok"]
    circuits = sorted({r["circuit"] for r in rows})
    lines = [
        "# E1 Benchmark Report",
        "",
        f"- Rows: {len(rows)}",
        f"- OK rows: {len(ok_rows)}",
        f"- Error rows: {len(error_rows)}",
        "",
        "## Circuit Status",
        "",
        "| Circuit | Version | Rows | Mean prove ms | Mean verify ms | Mean proof bytes | Notes |",
        "|---|---:|---:|---:|---:|---:|---|",
    ]
    for circuit in circuits:
        group = [r for r in ok_rows if r["circuit"] == circuit]
        any_row = next((r for r in rows if r["circuit"] == circuit), None)
        version = any_row.get("circuit_version", "") if any_row else ""
        note = any_row.get("note", "") if any_row else ""
        if group:
            prove = statistics.fmean(float(r["prove_ms"]) for r in group)
            verify = statistics.fmean(float(r["verify_ms"]) for r in group)
            proof = statistics.fmean(float(r["proof_bytes"]) for r in group)
            lines.append(f"| {circuit} | {version} | {len(group)} | {prove:.3f} | {verify:.3f} | {proof:.1f} | {note} |")
        else:
            lines.append(f"| {circuit} | {version} | 0 |  |  |  | {note} |")

    if error_rows:
        lines.extend(["", "## Errors", ""])
        for row in error_rows[:20]:
            lines.append(f"- seed={row['seed']} events={row['events_per_lot']} circuit={row['circuit']}: {row['note']}")

    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    rows = read_rows(args.raw)
    os.makedirs(args.out_dir, exist_ok=True)
    figs_dir = os.path.join(args.out_dir, "figs")
    os.makedirs(figs_dir, exist_ok=True)

    write_summary(rows, os.path.join(args.out_dir, "summary.csv"))
    for metric in PLOT_METRICS:
        write_svg_plot(rows, metric, os.path.join(figs_dir, f"{metric}.svg"))
    write_report(rows, os.path.join(args.out_dir, "report.md"))


if __name__ == "__main__":
    main()
