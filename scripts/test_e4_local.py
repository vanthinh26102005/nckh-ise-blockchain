#!/usr/bin/env python3
"""Integrity tests for the separate E4 local CPU evidence path."""
import copy
import json
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import e4_local
import e4_analyze
from policy_test_fixtures import receipt


def fixture():
    revision = "a" * 40
    policy_sha = "b" * 64
    pair = {"certificateRoot": "0x" + "11" * 32, "polygonCommitment": "0x" + "22" * 32}
    counts = [8, 9, 10, 11]
    transactions = [receipt(index=i, event_count=n, lot_count=1) for i, n in enumerate(counts)]
    workload = {"seed": 3, "np": 3, "shipment": 16, "epoch_s": 60, "epochs": 4,
                "offeredEvents": sum(counts), "offeredShipments": 4, "perEpochEvents": counts,
                "perEpochShipments": [1] * 4, "digest": "0x" + "33" * 32}
    policy = {"protocolVersion": 2, "sourceRevision": revision, **pair, "workloads": [workload],
              "leafVKeyDigest": "0x" + "44" * 32, "aggregateVKeyDigest": "0x" + "55" * 32,
              "aggregateProgramVKey": transactions[0]["programVKey"]}
    gate = {"status": "passed", "anvilReceiptStatus": 1, "gitCommit": revision,
            "protocolVersion": 2, "verificationKind": "sp1-groth16",
            "tamperedAggregateProofRejected": True, "tamperedAggregateValuesRejected": True,
            "transactionHash": "0x" + "cc" * 32,
            "policyProgramVKeyDigest": policy["leafVKeyDigest"],
            "aggregateProgramVKeyDigest": policy["aggregateVKeyDigest"]}
    epochs = [{"epochId": i + 1, "events": n, "shipments": 1, "lots": 1,
               "leafProvingMs": [100.0 + i], "aggregateProvingMs": 200.0 + i,
               "auditLatencyMs": [300.0 + i], "gasUsed": 1000.0,
               "calldataBytes": 320.0, "transactionHash": transactions[i]["transactionHash"]}
              for i, n in enumerate(counts)]
    run = {"validationKind": "e4-local-smoke", "pacing": "accelerated", "mode": "proposed",
           "sourceRevision": revision, "registryManifestSHA256": policy_sha,
           "host": socket.gethostname(), "protocolVersion": 2, "verificationKind": "sp1-groth16",
           "status": "ok", "epochs_completed": 4, "queue_drained": True,
           "workload_digest": workload["digest"], "offered_events": sum(counts),
           "committed_events": sum(counts), "offered_shipments": 4, "committed_shipments": 4,
           "elapsed_s": 900.0, "registryPair": pair, "epoch_samples": epochs,
           "transactions": transactions, "leaf_samples": [100.0 + i for i in range(4)],
           "aggregate_samples": [200.0 + i for i in range(4)],
           "audit_samples": [v for epoch in epochs for v in epoch["auditLatencyMs"]],
           "gas_samples": [1000.0] * 4, "calldata_samples": [320.0] * 4}
    return run, policy, gate, policy_sha


class TestLocalEvidence(unittest.TestCase):
    def test_four_shards_merge_and_cost_denominator(self):
        run, policy, gate, policy_sha = fixture()
        shards, manifest = e4_local.make_shards(run, policy, gate, policy_sha)
        merged = e4_local.merge(shards, manifest, policy, gate, policy_sha)
        report = e4_local.summarize(merged)
        self.assertEqual(report["events"], 38)
        self.assertEqual(report["gas_per_shipment"], 1000)
        self.assertEqual(report["calldata_bytes_per_shipment"], 320)
        self.assertFalse(report["formalComplete"])

    def test_duplicate_missing_and_partial_shards_rejected(self):
        run, policy, gate, policy_sha = fixture()
        shards, manifest = e4_local.make_shards(run, policy, gate, policy_sha)
        for bad in (shards[:-1], shards + [shards[0]], [shards[0]] * 4):
            with self.assertRaisesRegex(ValueError, "four unique"):
                e4_local.merge(bad, manifest, policy, gate, policy_sha)
        manifest["status"] = "censored"
        with self.assertRaisesRegex(ValueError, "partial"):
            e4_local.merge(shards, manifest, policy, gate, policy_sha)

    def test_source_host_policy_and_receipt_tampering_rejected(self):
        run, policy, gate, policy_sha = fixture()
        shards, manifest = e4_local.make_shards(run, policy, gate, policy_sha)
        for field, value in (("sourceRevision", "f" * 40), ("host", "other-host"),
                             ("policyManifestSHA256", "f" * 64)):
            bad = copy.deepcopy(shards)
            bad[2][field] = value
            with self.assertRaisesRegex(ValueError, "mismatch"):
                e4_local.merge(bad, manifest, policy, gate, policy_sha)
        bad = copy.deepcopy(shards)
        bad[2]["receipt"]["status"] = 0
        with self.assertRaisesRegex(ValueError, "successful Anvil"):
            e4_local.merge(bad, manifest, policy, gate, policy_sha)

    def test_samples_must_match_receipt_costs(self):
        for field, samples, value in (("gasUsed", "gas_samples", 123456.0),
                                      ("calldataBytes", "calldata_samples", 999.0)):
            with self.subTest(field=field):
                run, policy, gate, policy_sha = fixture()
                run["epoch_samples"][0][field] = value
                run[samples][0] = value
                with self.assertRaisesRegex(ValueError, "receipt costs"):
                    e4_local.make_shards(run, policy, gate, policy_sha)

    def test_analyze_cli_revalidates_receipts_and_trusted_inputs(self):
        for mutation in (None, "receipt", "source", "workload"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                run, policy, gate, _ = fixture()
                policy_bytes = json.dumps(policy).encode()
                policy_sha = e4_local.sha(policy_bytes)
                run["registryManifestSHA256"] = policy_sha
                rows, manifest = e4_local.make_shards(run, policy, gate, policy_sha)
                merged = e4_local.merge(rows, manifest, policy, gate, policy_sha)
                if mutation == "receipt": merged["transactions"][0]["status"] = 0
                if mutation == "source": merged["sourceRevision"] = "f" * 40
                if mutation == "workload": merged["workload_digest"] = "0x" + "ee" * 32
                (root / "policy.json").write_bytes(policy_bytes)
                (root / "gate.json").write_text(json.dumps(gate))
                (root / "merged.json").write_text(json.dumps(merged))
                result = subprocess.run([sys.executable, str(Path(e4_local.__file__)), "analyze",
                    "--merged", str(root / "merged.json"), "--policy", str(root / "policy.json"),
                    "--gate", str(root / "gate.json"), "--report", str(root / "report.json")],
                    text=True, capture_output=True)
                if mutation is None:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    report = json.loads((root / "report.json").read_text())
                    self.assertEqual(report["events"], 38)
                    self.assertEqual(report["gas_total"], 4000)
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse((root / "report.json").exists())

    def test_untrusted_workload_and_non_drained_run_rejected(self):
        run, policy, gate, policy_sha = fixture()
        run["queue_drained"] = False
        with self.assertRaisesRegex(ValueError, "drained"):
            e4_local.make_shards(run, policy, gate, policy_sha)
        run["queue_drained"] = True
        policy["workloads"][0]["perEpochEvents"][0] += 1
        with self.assertRaisesRegex(ValueError, "accounting"):
            e4_local.make_shards(run, policy, gate, policy_sha)

    def test_partial_run_keeps_completed_epochs_and_reason(self):
        run, policy, gate, policy_sha = fixture()
        run.update(status="censored", censor_reason="Fabric RPC ended", epochs_completed=2,
                   queue_drained=False, committed_events=17, committed_shipments=2)
        run["epoch_samples"] = run["epoch_samples"][:2]
        run["transactions"] = run["transactions"][:2]
        shards, manifest = e4_local.make_shards(run, policy, gate, policy_sha)
        self.assertEqual(len(shards), 2)
        self.assertEqual(manifest["censor_reason"], "Fabric RPC ended")
        with self.assertRaisesRegex(ValueError, "four unique"):
            e4_local.merge(shards, manifest, policy, gate, policy_sha)

    def test_protocol_keys_and_workload_mismatch_rejected(self):
        for mutation in ("protocol", "key", "receipt_key", "digest"):
            run, policy, gate, policy_sha = fixture()
            if mutation == "protocol": run["protocolVersion"] = 1
            if mutation == "key": gate["aggregateProgramVKeyDigest"] = "0x" + "fe" * 32
            if mutation == "receipt_key": run["transactions"][1]["leafVKeyDigest"] = "0x" + "fe" * 32
            if mutation == "digest": run["workload_digest"] = "0x" + "ee" * 32
            with self.assertRaises(ValueError):
                e4_local.make_shards(run, policy, gate, policy_sha)

    def test_formal_analyzer_rejects_local_evidence(self):
        record = {"executor": "real-e3", "validationKind": "e4-local-smoke", "run_id": "local",
                  "cell_id": "local", "phase": "screening", "np": 3, "shipment": 16, "epoch_s": 60,
                  "seed": 3, "status": "censored", "epochs_completed": 0, "queue_drained": False,
                  "transactions": [], "offered_events": None, "committed_events": None,
                  "offered_shipments": None, "committed_shipments": None}
        errors, _ = e4_analyze.validate([record], {"validationKind": "e4-local-smoke", "runs_total": 1}, [], True, True)
        self.assertTrue(any("local smoke evidence" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
