import json
import tempfile
import unittest
from pathlib import Path

from e3_analyze import BASELINES, analyze


class E3AnalyzeTests(unittest.TestCase):
    def make_runs(self, directory, manifest="same"):
        paths = {}
        for baseline in BASELINES:
            path = Path(directory) / f"{baseline}.jsonl"
            rows = [
                {"kind": "run", "baseline": baseline, "seed": 1, "mode": "accelerated-pilot",
                 "durationMin": 1.0, "lambdaEventsPerMin": 480.0, "producerCount": 125,
                 "expectedShipments": 2, "manifestSha256": manifest},
                {"kind": "shipment", "baseline": baseline, "seed": 1,
                 "shipmentId": "1", "status": "ok", "completedAtMs": 50000,
                 "eventCount": 8,
                 "auditLatencyMs": 100, "receipt": {"gasUsed": None if baseline == "fabric-only" else "1000",
                 "calldataBytes": None if baseline == "fabric-only" else 80}},
                {"kind": "shipment", "baseline": baseline, "seed": 1,
                 "shipmentId": "2", "status": "error", "completedAtMs": 52000,
                 "eventCount": 8,
                 "auditLatencyMs": None, "receipt": None},
                {"kind": "seed-end", "baseline": baseline, "seed": 1, "elapsedMs": 52000},
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            paths[baseline] = path
        return paths

    def test_pairing_and_failure_denominator(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = self.make_runs(directory)
            result = analyze(paths, expected_seeds=1, allow_pilot=True, bootstrap_iterations=100)
            self.assertEqual(result["perSeed"]["proposed"][1]["completionRate"], 0.5)
            self.assertEqual(result["perSeed"]["proposed"][1]["throughputEventsPerMin"], 8)
            self.assertEqual(result["perSeed"]["fabric-only"][1]["gasPerShipment"], None)
            self.assertEqual(len(result["pairedWilcoxonBonferroni"]["completionRate"]), 6)

    def test_rejects_different_manifest_and_pilot_as_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = self.make_runs(directory)
            with self.assertRaisesRegex(ValueError, "pilot"):
                analyze(paths, expected_seeds=1, allow_pilot=False, bootstrap_iterations=10)
            text = paths["proposed"].read_text().replace('"manifestSha256": "same"', '"manifestSha256": "other"')
            paths["proposed"].write_text(text)
            with self.assertRaisesRegex(ValueError, "same canonical manifest"):
                analyze(paths, expected_seeds=1, allow_pilot=True, bootstrap_iterations=10)


if __name__ == "__main__":
    unittest.main()
