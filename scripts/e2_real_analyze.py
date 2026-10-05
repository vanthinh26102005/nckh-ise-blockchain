#!/usr/bin/env python3
"""Audit policy-v2 E2 accounting. Incomplete/pilot data requires --allow-partial."""
import argparse
import hashlib
import json
import math
import random
import re
from pathlib import Path
from statistics import fmean
from e2_preflight import validate_record as validate_preflight_record

REPO = Path(__file__).resolve().parents[1]
HEX32 = re.compile(r"0x[0-9a-fA-F]{64}$")


def percentile(values, percentage):
    if not values:
        return None
    ordered = sorted(values)
    rank = percentage / 100 * (len(ordered) - 1)
    low, high = int(rank), min(int(rank) + 1, len(ordered) - 1)
    return ordered[low] + (ordered[high] - ordered[low]) * (rank - low)


def integer(record, key):
    value = record.get(key)
    if type(value) is not int or value < 0:
        raise ValueError(f"{key} must be a nonnegative integer")
    return value


def summarize(rows, scheduled_lots, scheduled_events, terminal=None):
    successes = [row for row in rows if row.get("status") == "ok"]
    latencies = [float(row["fabricAcknowledgementToReceiptMs"]) for row in successes]
    saturated = sum(row.get("failureStage") == "queue_saturated" for row in rows)
    return {
        "scheduledLots": scheduled_lots, "scheduledEvents": scheduled_events,
        "recordedLots": len(rows), "anchoredLots": len(successes),
        "failedLots": sum(row.get("status") != "ok" for row in rows),
        "unrecordedLots": max(0, scheduled_lots - len(rows)),
        "completionRate": len(successes) / scheduled_lots if scheduled_lots else 0.0,
        "saturatedLots": saturated, "saturationRate": saturated / scheduled_lots if scheduled_lots else 0.0,
        "medianMs": percentile(latencies, 50), "iqrMs": [percentile(latencies, 25), percentile(latencies, 75)],
        "p95Ms": percentile(latencies, 95), "p99Ms": percentile(latencies, 99),
        "meanMs": fmean(latencies) if latencies else None,
        "latenciesConditionedOnAnchoredLots": True,
        "terminal": terminal,
    }


def seed_confidence_intervals(per_seed):
    result = {}
    rng = random.Random(2)
    for metric in ("medianMs", "p95Ms", "p99Ms"):
        values = [row[metric] for row in per_seed.values() if row[metric] is not None]
        if len(values) < 2:
            result[metric] = None
            continue
        draws = [fmean(rng.choices(values, k=len(values))) for _ in range(2000)]
        result[metric] = [percentile(draws, 2.5), percentile(draws, 97.5)]
    return result


def analyze_records(records, *, policy_manifest=None, policy_manifest_bytes=None,
                    expected_seeds=30, allow_partial=False):
    runs = [row for row in records if row.get("kind") == "run"]
    if len(runs) != 1 or not records or records[0].get("kind") != "run":
        raise ValueError("exactly one leading run record is required")
    run = runs[0]
    if run.get("protocolVersion") != 2 or run.get("policyVersion") != 2:
        raise ValueError("historical runs are not policy-v2 evidence")
    run_id = integer(run, "runId")
    if any(row.get("runId") != run_id for row in records):
        raise ValueError("records have missing or mixed run IDs")
    seeds = integer(run, "seeds")
    if not seeds:
        raise ValueError("run must declare at least one seed")
    expected = set(range(1, seeds + 1))
    reasons = []
    for field in ("lambdaEventsPerMin", "durationMin"):
        if not isinstance(run.get(field), (int, float)) or not math.isfinite(run[field]) or run[field] <= 0:
            raise ValueError(f"invalid {field}")
    if run.get("mode") != "poisson-paced":
        reasons.append("accelerated/pilot pacing")
    if run["lambdaEventsPerMin"] != 480 or run["durationMin"] != 60 or seeds != 30 or seeds != expected_seeds:
        reasons.append("formal workload requires 480 events/min, 60 minutes and exactly 30 seeds")
    if not re.fullmatch(r"[0-9a-f]{40}", str(run.get("sourceRevision", ""))) or run.get("sourceDirty") is not False:
        reasons.append("source revision is missing or checkout is dirty/unreported")
    preflight=run.get("preflight")
    if preflight is None:
        reasons.append("read-only compute-host topology/hardware preflight is missing")
    else:
        validate_preflight_record(preflight)
        if preflight["sourceRevision"]!=run.get("sourceRevision") or preflight["computeHost"]!=run.get("computeHost") or preflight["software"]["sp1_version"]!=run.get("sp1SdkVersion"):
            raise ValueError("run binary source/host/SP1 differs from preflight provenance")
        encoded=json.dumps(preflight,separators=(",",":"),sort_keys=True,ensure_ascii=False).encode()
        if hashlib.sha256(encoded).hexdigest()!=run.get("preflightRecordSHA256"):
            raise ValueError("E2 header preflight record digest does not match its metadata")
    if policy_manifest is None or policy_manifest_bytes is None:
        reasons.append("trusted policy manifest was not supplied for independent accounting")
    else:
        if hashlib.sha256(policy_manifest_bytes).hexdigest() != run.get("registry_manifest_sha256"):
            raise ValueError("raw run does not match the supplied trusted policy manifest digest")
        if policy_manifest.get("protocolVersion") != 2 or policy_manifest.get("runId") != run_id:
            raise ValueError("trusted manifest protocol or run ID mismatch")
        for field in ("lambdaEventsPerMin", "durationMin", "seeds"):
            if policy_manifest.get(field) != run.get(field):
                raise ValueError("run workload differs from the trusted manifest")
        if policy_manifest.get("acceptedPolicies") != run.get("acceptedPolicies"):
            raise ValueError("run accepted policies differ from the trusted manifest")
        setup = [row for row in records if row.get("kind") == "setup"]
        if len(setup) != 1 or setup[0].get("policyProgramVKey") != policy_manifest.get("leafProgramVKey", policy_manifest.get("programVKey")):
            raise ValueError("run program key differs from the trusted manifest")
    plans, terminals, registries, lots = {}, {}, {}, {seed: [] for seed in expected}
    seen_lots = set()
    summaries = []
    for row in records:
        kind = row.get("kind")
        if kind in ("seed_plan", "seed_terminal", "registry", "lot"):
            seed = integer(row, "seed")
            if seed not in expected:
                raise ValueError("record seed is outside declared run seeds")
            if kind in ("seed_plan", "seed_terminal", "registry"):
                target = {"seed_plan": plans, "seed_terminal": terminals, "registry": registries}[kind]
                if seed in target:
                    raise ValueError(f"duplicate {kind} for seed {seed}")
                target[seed] = row
            else:
                lot_id = integer(row, "lotId")
                if lot_id in seen_lots:
                    raise ValueError("lot IDs repeat across this workload's seed snapshots")
                seen_lots.add(lot_id)
                if not 8 <= integer(row, "eventCount") <= 64:
                    raise ValueError("lot event count outside policy range")
                if row.get("status") not in ("ok", "failed", "not_offered"):
                    raise ValueError("unknown lot terminal status")
                if row["status"] == "ok":
                    value = row.get("fabricAcknowledgementToReceiptMs")
                    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
                        raise ValueError("successful lot must have finite nonnegative latency")
                    if row.get("protocolVersion") != 2 or row.get("verificationKind") != "sp1-groth16" or row.get("policyValid") is not True:
                        raise ValueError("successful lot lacks policy-v2 real-proof receipt metadata")
                    if not HEX32.fullmatch(str(row.get("transactionHash", ""))):
                        raise ValueError("successful lot is missing its mined transaction hash")
                    if not re.fullmatch(r"[0-9]+", str(row.get("gasUsed", ""))):
                        raise ValueError("successful lot is missing numeric gas usage")
                    pair = {key: row.get(key) for key in ("certificateRoot", "polygonCommitment")}
                    if pair not in run.get("acceptedPolicies", []):
                        raise ValueError("receipt policy pair outside trusted run configuration")
                elif not row.get("failureStage") or not row.get("error"):
                    raise ValueError("unsuccessful lot must preserve failure stage and reason")
                lots[seed].append(row)
        elif kind == "summary":
            summaries.append(row)
        elif kind not in ("run", "setup"):
            raise ValueError(f"unknown record kind {kind}")
    if len(summaries) != 1 or records[-1].get("kind") != "summary":
        reasons.append("missing final run summary (truncated run)")
    elif summaries[0].get("status") != "complete":
        reasons.append("run summary reports incomplete workload")
    configured_seeds = {}
    if policy_manifest is not None:
        for configured in policy_manifest.get("workloadSeeds", []):
            seed = integer(configured, "seed")
            if seed in configured_seeds:
                raise ValueError("trusted manifest repeats workload seed")
            configured_seeds[seed] = configured
        if set(configured_seeds) != expected:
            raise ValueError("trusted manifest does not cover the declared workload seeds")
    per_seed = {}
    for seed in sorted(expected):
        plan, terminal, rows = plans.get(seed), terminals.get(seed), lots[seed]
        if plan is None:
            reasons.append(f"seed {seed}: missing workload plan")
            scheduled_lots = terminal.get("scheduledLots", len(rows)) if terminal else len(rows)
            scheduled_events = terminal.get("scheduledEvents", sum(row["eventCount"] for row in rows)) if terminal else sum(row["eventCount"] for row in rows)
        else:
            scheduled_lots, scheduled_events = integer(plan, "scheduledLots"), integer(plan, "scheduledEvents")
            if not 0.95 * run["lambdaEventsPerMin"] * run["durationMin"] <= scheduled_events <= 1.05 * run["lambdaEventsPerMin"] * run["durationMin"]:
                reasons.append(f"seed {seed}: offered plan volume is not plausible for the full Poisson workload")
            if plan.get("lastScheduledAtMs", 0) < run["durationMin"] * 60_000 * 0.95:
                reasons.append(f"seed {seed}: schedule ends before the declared workload horizon")
            if policy_manifest is not None:
                for field in ("scheduledLots", "scheduledEvents", "scheduleSha256", "lastScheduledAtMs"):
                    if configured_seeds[seed].get(field) != plan.get(field):
                        raise ValueError("run workload plan differs from trusted prebuilt schedule")
        if len(rows) != scheduled_lots or sum(row["eventCount"] for row in rows) != scheduled_events:
            reasons.append(f"seed {seed}: some scheduled lots/events have no terminal row")
        if terminal is None:
            reasons.append(f"seed {seed}: missing seed terminal")
        else:
            for field, actual in (("scheduledLots", scheduled_lots), ("scheduledEvents", scheduled_events),
                                  ("anchoredLots", sum(row["status"] == "ok" for row in rows)),
                                  ("anchoredEvents", sum(row["eventCount"] for row in rows if row["status"] == "ok")),
                                  ("saturatedLots", sum(row.get("failureStage") == "queue_saturated" for row in rows))):
                if integer(terminal, field) != actual:
                    raise ValueError(f"seed {seed}: terminal {field} contradicts lot accounting")
            for chain in (("scheduledLots", "offeredLots", "committedLots", "queuedLots", "receivedLots", "provedLots", "anchoredLots"),
                          ("scheduledEvents", "offeredEvents", "committedEvents", "provedEvents", "anchoredEvents")):
                counts = [integer(terminal, key) for key in chain]
                if any(a < b for a, b in zip(counts, counts[1:])):
                    raise ValueError("terminal progress counters are not monotonic")
            if terminal.get("status") != "complete" or terminal.get("failure") or any(row["status"] != "ok" for row in rows):
                reasons.append(f"seed {seed}: failed, saturated or incomplete lots")
            if terminal.get("offeredEvents") != scheduled_events or terminal.get("offeredLots") != scheduled_lots:
                reasons.append(f"seed {seed}: full planned workload was not offered")
            capacity = run.get("queueCapacity")
            if type(capacity) is not int or capacity <= 0 or integer(terminal, "maxQueueDepth") > capacity:
                raise ValueError("bounded queue depth exceeds configured capacity")
            producer_duration = terminal.get("producerDurationSec")
            if not isinstance(producer_duration, (int, float)) or not math.isfinite(producer_duration) or producer_duration < 0:
                raise ValueError("seed terminal needs actual producer duration")
            if producer_duration * 1000 < (plan.get("lastScheduledAtMs", 0) if plan else 0):
                reasons.append(f"seed {seed}: actual producer pacing stopped before the scheduled horizon")
        if seed not in registries:
            reasons.append(f"seed {seed}: missing prebuilt registry record")
        elif policy_manifest is not None:
            pair = {key: registries[seed].get(key) for key in ("certificateRoot", "polygonCommitment")}
            if pair != configured_seeds[seed].get("policy"):
                raise ValueError("run registry differs from trusted workload policy")
        per_seed[str(seed)] = summarize(rows, scheduled_lots, scheduled_events, terminal)
    if reasons and not allow_partial:
        raise ValueError("not formal E2 evidence: " + "; ".join(reasons[:4]) + "; use --allow-partial for an explicitly nonformal report")
    all_rows = [row for group in lots.values() for row in group]
    return {
        "experiment": "E2 real Fabric acknowledgement to Anvil receipt", "protocolVersion": 2,
        "evidenceClass": "nonformal-partial" if reasons else "formal", "nonformalReasons": reasons, "run": run,
        "aggregate": summarize(all_rows, sum(row["scheduledLots"] for row in per_seed.values()), sum(row["scheduledEvents"] for row in per_seed.values())),
        "perSeed": per_seed, "seedStatisticCI95Ms": seed_confidence_intervals(per_seed),
        "confidenceIntervalMethod": "2000 percentile bootstrap samples of seeds, mean of each per-seed statistic, deterministic seed 2; missing-latency seeds excluded only from latency CI",
        "verificationBoundary": "accounting and trusted manifest metadata; runner verifies SP1/Anvil receipts, analyzer does not re-query the chain",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--policy-manifest", type=Path)
    parser.add_argument("--expected-seeds", type=int, default=30)
    parser.add_argument("--allow-partial", action="store_true")
    parser.add_argument("--allow-accelerated", action="store_true", help="Compatibility alias for explicit nonformal pilot analysis")
    args = parser.parse_args()
    if args.out.resolve().is_relative_to(REPO) or args.out.exists():
        parser.error("--out must be a new file outside Git")
    raw_manifest = args.policy_manifest.read_bytes() if args.policy_manifest else None
    try:
        records = [json.loads(line) for line in args.raw.read_text().splitlines() if line.strip()]
        result = analyze_records(records, policy_manifest=json.loads(raw_manifest) if raw_manifest else None,
                                 policy_manifest_bytes=raw_manifest, expected_seeds=args.expected_seeds,
                                 allow_partial=args.allow_partial or args.allow_accelerated)
    except (ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        output.write(json.dumps(result, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
