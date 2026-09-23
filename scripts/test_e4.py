#!/usr/bin/env python3
"""Tests for the E4 harness (plan / run / analyze). Stdlib unittest, no third-party deps.

Run: python3 scripts/test_e4.py   (or `make e4-test`)
"""

import importlib.util
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent


def _load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


e4_plan = _load("e4_plan")
e4_run = _load("e4_run")
e4_analyze = _load("e4_analyze")


class TestPlan(unittest.TestCase):
    def test_counts_match_issue(self):
        rows = list(e4_plan.generate_rows(e4_plan.PHASES))
        screening = [r for r in rows if r["phase"] == "screening"]
        confirmation = [r for r in rows if r["phase"] == "confirmation"]
        # 5x4x4 x1 seed, 3x3x3 x10 seed
        self.assertEqual(len(screening), 80)
        self.assertEqual(len(confirmation), 270)

    def test_seed_determinism(self):
        a = list(e4_plan.generate_rows(e4_plan.PHASES))
        b = list(e4_plan.generate_rows(e4_plan.PHASES))
        self.assertEqual([r["run_id"] for r in a], [r["run_id"] for r in b])

    def test_offered_load(self):
        row = next(r for r in e4_plan.generate_rows({"screening": e4_plan.PHASES["screening"]})
                   if r["np"] == 2000)
        self.assertAlmostEqual(row["offered_load_epm"], 3.84 * 2000, places=3)

    def test_epochs_target(self):
        self.assertTrue(all(r["epochs_target"] == 4
                            for r in e4_plan.generate_rows(e4_plan.PHASES)))


class TestStat(unittest.TestCase):
    def test_empty(self):
        s = e4_run.stat([])
        self.assertEqual(s["count"], 0)

    def test_percentiles(self):
        s = e4_run.stat(list(range(1, 101)))  # 1..100
        self.assertEqual(s["min"], 1)
        self.assertEqual(s["max"], 100)
        self.assertAlmostEqual(s["p50"], 50.5, places=1)
        self.assertTrue(94 <= s["p95"] <= 96)


class TestMockExecutor(unittest.TestCase):
    def test_deterministic(self):
        row = {"np": "500", "shipment": "64", "epoch_s": "120", "seed": "12345"}
        r1 = e4_run.MockExecutor().run(dict(row))
        r2 = e4_run.MockExecutor().run(dict(row))
        self.assertEqual(r1["offered_events"], r2["offered_events"])
        self.assertEqual(r1["throughput_eps"], r2["throughput_eps"])

    def test_real_executor_blocked(self):
        with self.assertRaises(NotImplementedError):
            e4_run.RealE3Executor().run({"np": "1", "shipment": "8", "epoch_s": "30", "seed": "1"})


def _cell_record(cell_id, np_, sh, ep, seed, status="ok", epc=4, executor="real-e3", split_host=False):
    z = {"count": 1, "mean": 1.0, "p50": 1.0, "p95": 1.0, "p99": 1.0, "min": 1.0, "max": 1.0, "sum": 1.0}
    return {
        "kind": "cell", "run_id": f"{cell_id}#s{seed}", "phase": "confirmation", "cell_id": cell_id,
        "np": np_, "shipment": sh, "epoch_s": ep, "seed": seed, "executor": executor, "status": status,
        "epochs_completed": epc, "queue_drained": status == "ok", "throughput_eps": 10.0,
        "completion_rate": 1.0 if status == "ok" else 0.5, "leaf_proving_ms": z,
        "aggregate_proving_ms": z, "audit_latency_ms": z, "gas_used": z, "calldata_bytes": z,
        "split_host": split_host,
    }


class TestAnalyzeValidation(unittest.TestCase):
    def _validate(self, records, manifest, require_real=False, require_single_host=False):
        cells = e4_analyze.aggregate_cells(records)
        return e4_analyze.validate(records, manifest, cells, require_real, require_single_host)

    def test_require_real_rejects_mock(self):
        recs = [_cell_record("c", 1, 8, 30, 0, executor="mock")]
        errors, _ = self._validate(recs, {"seeds_per_cell": 1}, require_real=True)
        self.assertTrue(any("mock" in e for e in errors))

    def test_wrong_seed_count(self):
        recs = [_cell_record("c", 1, 8, 30, s) for s in range(3)]  # 3 seeds
        errors, _ = self._validate(recs, {"seeds_per_cell": 10})  # expected 10
        self.assertTrue(any("expected 10" in e for e in errors))

    def test_split_host_rejected(self):
        recs = [_cell_record("c", 1, 8, 30, 0, split_host=True)]
        errors, _ = self._validate(recs, {"seeds_per_cell": 1}, require_single_host=True)
        self.assertTrue(any("split-host" in e for e in errors))

    def test_epochs_not_four_rejected(self):
        recs = [_cell_record("c", 1, 8, 30, 0, status="ok", epc=3)]
        errors, _ = self._validate(recs, {"seeds_per_cell": 1})
        self.assertTrue(any("epochs_completed" in e for e in errors))

    def test_saturated_cells_kept(self):
        recs = [_cell_record("c", 1, 8, 30, 0, status="saturated", epc=2)]
        cells = e4_analyze.aggregate_cells(recs)
        self.assertEqual(cells[0]["n_saturated"], 1)
        # saturated must not be dropped from the aggregate
        self.assertEqual(cells[0]["runs"], 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
