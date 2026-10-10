import hashlib
import json
import tempfile
import unittest
import subprocess
import sys
from pathlib import Path

import e2_real_analyze as analyzer
from e4_preflight import FABRIC_CONTAINERS


def complete_fixture(seeds=30, lots_per_seed=450):
    pair = {"certificateRoot": "0x" + "11" * 32, "polygonCommitment": "0x" + "22" * 32}
    workload = [{"seed": seed, "scheduledEvents": lots_per_seed * 64,
                 "scheduledLots": lots_per_seed, "scheduleSha256": "a" * 64,
                 "lastScheduledAtMs": 3_599_000, "policy": pair}
                for seed in range(1, seeds + 1)]
    manifest = {"protocolVersion": 2, "runId": 1, "leafProgramVKey": "0x" + "33" * 32,
                "sourceRevision":"b"*40,"sourceDirty":False,"validationKind":"formal",
                "lambdaEventsPerMin": 480.0, "durationMin": 60.0, "seeds": seeds,
                "acceptedPolicies": [pair], "workloadSeeds": workload}
    raw_manifest = json.dumps(manifest).encode()
    preflight={"kind":"e2_preflight","protocolVersion":2,"sourceRevision":"b"*40,"computeHost":"fixture",
        "hardware":{"hostname":"fixture","cpu_model":"fixture","cpu_logical":8,"ram_bytes":1000000,"gpu_model_driver":"fixture","cuda_toolkit":"fixture","os":"fixture"},
        "topology":{"fabricContainers":{name:{"id":"fixture","image":"fixture"} for name in FABRIC_CONTAINERS},"anvilChainId":"0x7a69","anvilClientVersion":"fixture"},
        "software":{"sp1_version":"6.3.1","rustc":"fixture","cargo":"fixture"},
        "fabricGateway":"http://127.0.0.1:8080","anvilRpc":"http://127.0.0.1:8545","anchorServer":"http://127.0.0.1:8546"}
    rows = [{"kind": "run", "protocolVersion": 2, "policyVersion": 2, "runId": 1,
             "validationKind":"formal",
             "sourceRevision": "b" * 40, "sourceDirty": False, "mode": "poisson-paced", "lambdaEventsPerMin": 480.0,
             "durationMin": 60.0, "seeds": seeds, "registry_manifest_sha256": hashlib.sha256(raw_manifest).hexdigest(),
             "queueCapacity": 8, "acceptedPolicies": [pair]}, {"kind": "setup", "runId": 1, "policyProgramVKey": manifest["leafProgramVKey"]}]
    rows[0].update(preflight=preflight,computeHost="fixture",sp1SdkVersion="6.3.1",
        preflightRecordSHA256=hashlib.sha256(json.dumps(preflight,separators=(",",":"),sort_keys=True).encode()).hexdigest())
    for plan in workload:
        seed = plan["seed"]
        rows.append({"kind": "seed_plan", "runId": 1, **{key: val for key, val in plan.items() if key != "policy"}})
        rows.append({"kind": "registry", "runId": 1, "seed": seed, **pair})
        for ordinal in range(1, lots_per_seed + 1):
            rows.append({"kind": "lot", "runId": 1, "seed": seed, "lotId": 1_000_000_000 + seed * 1_000_000 + ordinal,
                         "status": "ok", "eventCount": 64, "fabricAcknowledgementToReceiptMs": float(ordinal),
                         "transactionHash": "0x" + f"{seed * 1000 + ordinal:064x}", "gasUsed": "100000",
                         "protocolVersion": 2, "verificationKind": "sp1-groth16", "policyValid": True,
                         "fabricReadback":True, **pair})
        rows.append({"kind": "seed_terminal", "runId": 1, "seed": seed, "status": "complete",
                     "scheduledEvents": lots_per_seed * 64, "scheduledLots": lots_per_seed,
                     "offeredEvents": lots_per_seed * 64, "offeredLots": lots_per_seed,
                     "committedEvents": lots_per_seed * 64, "committedLots": lots_per_seed,
                     "readbackEvents": lots_per_seed * 64, "readbackLots": lots_per_seed,
                     "queuedLots": lots_per_seed, "receivedLots": lots_per_seed, "provedLots": lots_per_seed,
                     "provedEvents": lots_per_seed * 64, "anchoredLots": lots_per_seed, "anchoredEvents": lots_per_seed * 64,
                     "saturatedLots": 0, "maxQueueDepth": 8, "failure": None, "durationSec": 3601.0,
                     "producerDurationSec": 3599.1})
    rows.append({"kind": "summary", "runId": 1, "status": "complete"})
    return rows, manifest, raw_manifest


def local_fixture():
    rows, manifest, _ = complete_fixture(seeds=1, lots_per_seed=1)
    run = rows[0]
    run.update(validationKind="local-smoke", mode="accelerated-pilot", prover="cpu",
               lambdaEventsPerMin=16.0, durationMin=1.0)
    preflight = run["preflight"]
    preflight.update(validationKind="local-smoke", prover="cpu",
                     gpuUnavailableReason="CPU-only local smoke")
    preflight["hardware"].update(gpu_model_driver="N/A", cuda_toolkit="N/A")
    run["preflightRecordSHA256"] = hashlib.sha256(json.dumps(preflight,separators=(",",":"),sort_keys=True).encode()).hexdigest()
    manifest.update(validationKind="local-smoke", lambdaEventsPerMin=16.0, durationMin=1.0)
    manifest["workloadSeeds"][0].update(scheduledEvents=9, lastScheduledAtMs=55000)
    plan = next(row for row in rows if row.get("kind") == "seed_plan")
    plan.update(scheduledEvents=9, lastScheduledAtMs=55000)
    lot = next(row for row in rows if row.get("kind") == "lot")
    lot["eventCount"] = 9
    terminal = next(row for row in rows if row.get("kind") == "seed_terminal")
    for key in ("scheduledEvents","offeredEvents","committedEvents","readbackEvents","provedEvents","anchoredEvents"):
        terminal[key] = 9
    terminal["producerDurationSec"] = 1.0
    raw_manifest = json.dumps(manifest).encode()
    run["registry_manifest_sha256"] = hashlib.sha256(raw_manifest).hexdigest()
    return rows, manifest, raw_manifest


class E2AccountingTests(unittest.TestCase):
    def test_local_cpu_smoke_passes_only_local_gate(self):
        rows, manifest, raw = local_fixture()
        result = analyzer.analyze_records(rows, policy_manifest=manifest,
                                          policy_manifest_bytes=raw, expected_seeds=1, allow_partial=True)
        self.assertEqual(result["evidenceClass"], "local-smoke")
        self.assertTrue(result["localComplete"])
        self.assertFalse(result["formalComplete"])
        with self.assertRaisesRegex(ValueError, "never formal"):
            analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw,
                                     expected_seeds=1)
        with self.assertRaisesRegex(ValueError, "formal GPU gate"):
            analyzer.validate_preflight_record(rows[0]["preflight"])

    def test_local_missing_terminal_and_source_mismatch_are_rejected(self):
        rows, manifest, raw = local_fixture()
        partial = [row for row in rows if row.get("kind") != "seed_terminal"]
        result = analyzer.analyze_records(partial, policy_manifest=manifest,
                                          policy_manifest_bytes=raw, expected_seeds=1, allow_partial=True)
        self.assertFalse(result["localComplete"])
        rows[0]["sourceRevision"] = "c"*40
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, policy_manifest=manifest,
                                     policy_manifest_bytes=raw, expected_seeds=1, allow_partial=True)

    def test_success_cannot_claim_a_lot_without_fabric_readback(self):
        rows, manifest, raw = local_fixture()
        lot = next(row for row in rows if row.get("kind") == "lot")
        lot.pop("fabricReadback")
        with self.assertRaisesRegex(ValueError, "readback evidence"):
            analyzer.analyze_records(rows, policy_manifest=manifest,
                                     policy_manifest_bytes=raw, expected_seeds=1, allow_partial=True)

    def test_complete_formal_run_reports_p99_and_seed_bootstrap_interval(self):
        rows, manifest, raw = complete_fixture()
        result = analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw)
        self.assertEqual(result.get("evidenceClass"), "formal")
        self.assertEqual(result["aggregate"]["completionRate"], 1.0)
        self.assertEqual(result["aggregate"]["p99Ms"], 446.0)
        self.assertEqual(result["perSeed"]["1"]["p99Ms"], 445.51)
        self.assertEqual(result["seedStatisticCI95Ms"]["medianMs"], [225.5, 225.5])

    def test_failed_and_saturated_lots_remain_in_completion_denominator(self):
        rows, manifest, raw = complete_fixture(seeds=1, lots_per_seed=2)
        lot = next(row for row in rows if row.get("kind") == "lot")
        lot.update(status="failed", failureStage="queue_saturated", error="full")
        lot.pop("fabricAcknowledgementToReceiptMs")
        terminal = next(row for row in rows if row.get("kind") == "seed_terminal")
        terminal.update(status="partial", anchoredLots=1, anchoredEvents=64,
                        provedLots=1, provedEvents=64, queuedLots=1, receivedLots=1, saturatedLots=1)
        rows[-1]["status"] = "partial"
        result = analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw, allow_partial=True)
        self.assertEqual(result["evidenceClass"], "nonformal-partial")
        self.assertEqual(result["aggregate"]["completionRate"], 0.5)
        self.assertEqual(result["aggregate"]["saturationRate"], 0.5)
        self.assertEqual(result["aggregate"]["failedLots"], 1)
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw)

    def test_missing_terminal_truncation_and_low_volume_cannot_be_formal(self):
        rows, manifest, raw = complete_fixture(seeds=1, lots_per_seed=2)
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw)
        rows = [row for row in rows if row.get("kind") != "seed_terminal"]
        result = analyzer.analyze_records(rows, allow_partial=True)
        self.assertEqual(result["evidenceClass"], "nonformal-partial")
        self.assertIn("missing seed terminal", " ".join(result["nonformalReasons"]))

    def test_duplicate_ids_counter_lies_and_wrong_manifest_are_rejected(self):
        rows, manifest, raw = complete_fixture(seeds=1, lots_per_seed=2)
        terminal = next(row for row in rows if row.get("kind") == "seed_terminal")
        terminal["anchoredLots"] = 1
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, allow_partial=True)
        rows, _, _ = complete_fixture(seeds=1, lots_per_seed=2)
        lots = [row for row in rows if row.get("kind") == "lot"]
        lots[1]["lotId"] = lots[0]["lotId"]
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, allow_partial=True)
        rows, manifest, raw = complete_fixture(seeds=1, lots_per_seed=2)
        with self.assertRaises(ValueError):
            analyzer.analyze_records(rows, policy_manifest=manifest, policy_manifest_bytes=raw + b" ", allow_partial=True)

    def test_cli_keeps_existing_report_and_emits_explicit_partial_status(self):
        rows, manifest, raw = complete_fixture(seeds=1, lots_per_seed=2)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw_path, manifest_path, report_path = root / "raw.jsonl", root / "policy.json", root / "report.json"
            raw_path.write_text("".join(json.dumps(row) + "\n" for row in rows))
            manifest_path.write_bytes(raw)
            command = [sys.executable, str(Path(analyzer.__file__)), "--raw", str(raw_path),
                       "--policy-manifest", str(manifest_path), "--out", str(report_path), "--allow-partial"]
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
            before = report_path.read_bytes()
            self.assertEqual(json.loads(before)["evidenceClass"], "nonformal-partial")
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual(report_path.read_bytes(), before)

    def test_preflight_host_revision_topology_disagreement_is_rejected(self):
        rows,manifest,raw=complete_fixture(seeds=1,lots_per_seed=2)
        rows[0]["computeHost"]="another-node"
        with self.assertRaisesRegex(ValueError,"preflight provenance"):
            analyzer.analyze_records(rows,allow_partial=True)
        rows[0]["computeHost"]="fixture"
        rows[0]["preflight"]["topology"]["fabricContainers"].pop(FABRIC_CONTAINERS[0])
        with self.assertRaisesRegex(ValueError,"observed"):
            analyzer.analyze_records(rows,allow_partial=True)


if __name__ == "__main__":
    unittest.main()
