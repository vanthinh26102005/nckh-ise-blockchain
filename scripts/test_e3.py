#!/usr/bin/env python3
"""Small deterministic checks for E3 comparison statistics and strict coverage."""
import unittest

from e3_analyze import analyze, bootstrap_mean_ci, wilcoxon_exact
from e3_run import MODES, mode_order
from policy_test_fixtures import receipt


class E3AnalysisTests(unittest.TestCase):
    def test_mode_order_rotates_every_mode_through_every_position(self):
        for position in range(4):
            self.assertEqual({mode_order(seed)[position] for seed in range(4)}, set(MODES))

    def test_historical_records_cannot_be_formal_v2(self):
        row = {"mode": "fabric-only", "seed": 0, "np": 125,
               "lambda_events_per_min": 480, "duration_min": 60, "shipment": 64,
               "epochs_target": 60, "git_commit": "abc", "gate_transaction_hash": "0x1",
               "prover": "cuda", "measurement": {"status": "censored", "offered_events": 10}}
        with self.assertRaisesRegex(ValueError, "policy-v2"):
            analyze([row])
        self.assertFalse(analyze([row], allow_partial=True)["formal"])

    def test_wilcoxon_exact_all_positive_six_pairs(self):
        self.assertAlmostEqual(wilcoxon_exact([2, 3, 4], [1, 1, 1]), 0.25)

    def test_wilcoxon_ties_and_zeros(self):
        self.assertEqual(wilcoxon_exact([1, 2], [1, 2]), 1)
        self.assertEqual(wilcoxon_exact([2, 1], [1, 2]), 1)

    def test_bootstrap_constant(self):
        self.assertEqual(bootstrap_mean_ci([7.0] * 30), [7.0, 7.0])

    def test_formal_rejects_partial(self):
        row = {"mode": "proposed", "seed": 0, "np": 125,
               "lambda_events_per_min": 480, "duration_min": 60, "shipment": 64,
               "epochs_target": 60, "git_commit": "abc", "gate_transaction_hash": "0x1",
               "prover": "cuda", "measurement": {"status": "censored", "offered_events": 10}}
        with self.assertRaisesRegex(ValueError, "policy-v2"):
            analyze([row])

    def test_complete_synthetic_matrix_exercises_six_paired_tests(self):
        records = []
        for seed in range(30):
            for index, mode in enumerate(MODES):
                count = 0 if mode == "fabric-only" else 60 if mode == "proposed" else 450
                measured = {"status": "ok", "epochs_completed": 60, "queue_drained": True,
                            "offered_events": 28800, "committed_events": 28800,
                            "offered_shipments": 450, "committed_shipments": 450,
                            "elapsed_s": 3600 + index * 10 + seed,
                            "audit_samples": [100 + index] * 450, "gas_samples": [1000] * count,
                            "calldata_samples": [320] * count,
                            "protocolVersion": 2, "verificationKind": "sp1-groth16",
                            "sourceRevision":"b"*40,"registryManifestSHA256":"a"*64,
                            "workload_digest": f"{seed:064x}",
                            "transactions": [receipt("EpochAggregated" if mode == "proposed" else "TokenMinted" if mode == "vecro-adapted" else "hash", i, 480 if mode == "proposed" else 64, 8 if mode == "proposed" else 1, seed*4+index) for i in range(count)]}
                records.append({"protocolVersion": 2, "registry_manifest_sha256": "a" * 64,
                                "acceptedPolicies": [{"certificateRoot": "0x" + "11" * 32, "polygonCommitment": "0x" + "22" * 32}],
                                "hardware": {"hostname": "fixture", "cpu_model": "fixture", "cpu_logical": 2, "ram_bytes": 1000, "gpu_model_driver": "fixture", "cuda_toolkit": "fixture", "os": "fixture"},
                                "ledgerIsolation":{"protocolVersion":2,"freshLedger":True,"sourceRevision":"b"*40,
                                    "seed":seed,"mode":mode,"computeHost":"fixture","order":mode_order(seed).index(mode),
                                    "namespace":f"seed-{seed}-{mode}","runtimePath":f"/tmp/seed-{seed}-{mode}"},
                                "software": {"sp1_version": "fixture", "fabric_images": {"peer": "fixture"}, "anvil_version": "fixture"},
                                "mode_order": mode_order(seed), "mode_order_index": mode_order(seed).index(mode), "mode": mode, "seed": seed, "np": 125,
                                "lambda_events_per_min": 480, "duration_min": 60,
                                "shipment": 64, "epochs_target": 60, "git_commit": "b"*40,
                                "gate_transaction_hash": "0x1", "prover": "cuda",
                                "trustedWorkload": {"seed":seed,"np":125,"shipment":64,"epoch_s":60,"epochs":60,
                                    "offeredEvents":28800,"offeredShipments":450,"digest":f"0x{seed:064x}"},
                                "measurement": measured})
        result = analyze(records)
        self.assertTrue(result["formal"])
        self.assertEqual(len(result["paired_tests"]), 6)
        records[0]["ledgerIsolation"]["freshLedger"] = False
        with self.assertRaisesRegex(ValueError,"ledgerIsolation"):analyze(records)
        records[0]["ledgerIsolation"]["freshLedger"] = True
        records[0]["measurement"]["sourceRevision"] = "c"*40
        with self.assertRaisesRegex(ValueError,"provenance"):analyze(records)
        records[0]["measurement"]["sourceRevision"] = "b"*40
        records[0]["measurement"]["elapsed_s"] = 10
        with self.assertRaisesRegex(ValueError, "elapsed"):
            analyze(records)
        records[0]["measurement"]["elapsed_s"] = 3600
        records[0]["measurement"]["offered_events"] = 480
        records[0]["measurement"]["committed_events"] = 480
        with self.assertRaisesRegex(ValueError, "trusted workload"):
            analyze(records)
        records[0]["measurement"]["offered_events"] = records[0]["measurement"]["committed_events"] = 28800
        records[-1]["measurement"]["workload_digest"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "trusted workload"):
            analyze(records)


if __name__ == "__main__":
    unittest.main()
