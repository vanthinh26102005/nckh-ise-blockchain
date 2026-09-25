#!/usr/bin/env python3
"""Small deterministic checks for E3 comparison statistics and strict coverage."""
import unittest

from e3_analyze import analyze, bootstrap_mean_ci, wilcoxon_exact
from e3_run import MODES


class E3AnalysisTests(unittest.TestCase):
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
        with self.assertRaisesRegex(ValueError, "all 4 modes x 30"):
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
                            "transactions": [{"transactionHash": "0x1", "gasUsed": "1000"}] * count}
                records.append({"mode": mode, "seed": seed, "np": 125,
                                "lambda_events_per_min": 480, "duration_min": 60,
                                "shipment": 64, "epochs_target": 60, "git_commit": "abc",
                                "gate_transaction_hash": "0x1", "prover": "cuda",
                                "measurement": measured})
        result = analyze(records)
        self.assertTrue(result["formal"])
        self.assertEqual(len(result["paired_tests"]), 6)


if __name__ == "__main__":
    unittest.main()
