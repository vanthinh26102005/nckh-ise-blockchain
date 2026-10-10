"""Local paired E3 evidence must remain separate from formal comparison data."""
import copy
import unittest

from e3_analyze import analyze as analyze_formal
from e3_local_analyze import summarize
from e3_run import MODES
from e4_preflight import mode_order
from policy_test_fixtures import receipt


def local_records():
    rows = []
    for index, mode in enumerate(MODES):
        tx = [] if mode == "fabric-only" else [receipt(
            "hash" if mode == "hash-on-chain" else "TokenMinted" if mode == "vecro-adapted" else "EpochAggregated",
            event_count=9, lot_count=1, namespace=index)]
        measured = {"status": "ok", "validationKind": "local-smoke", "pacing": "accelerated",
                    "sourceRevision": "c" * 40, "registryManifestSHA256": "a" * 64,
                    "workload_digest": "0x" + "b" * 64, "epochs_completed": 1, "queue_drained": True,
                    "offered_events": 9, "committed_events": 9,
                    "offered_shipments": 1, "committed_shipments": 1,
                    "elapsed_s": 10 + index, "audit_samples": [100 + index],
                    "gas_samples": [] if mode == "fabric-only" else [1000],
                    "calldata_samples": [] if mode == "fabric-only" else [320],
                    "transactions": tx}
        rows.append({"protocolVersion": 2, "validationKind": "local-smoke", "mode": mode,
                     "seed": 1, "prover": "cpu", "git_commit": "c" * 40,
                     "gate_transaction_hash": "0x" + "d" * 64,
                     "registry_manifest_sha256": "a" * 64,
                     "acceptedPolicies": [{"certificateRoot": "0x" + "11" * 32,
                                           "polygonCommitment": "0x" + "22" * 32}],
                     "trustedWorkload": {"digest": "0x" + "b" * 64,
                                         "offeredEvents": 9, "offeredShipments": 1},
                     "hardware": {"hostname": "fixture"},
                     "mode_order": mode_order(1), "mode_order_index": mode_order(1).index(mode),
                     "ledgerIsolation": {"freshLedger": True, "firstMeasuredEventAbsentBeforeRun": True,
                         "sourceRevision": "c" * 40, "mode": mode, "seed": 1,
                         "computeHost": "fixture", "order": mode_order(1).index(mode),
                         "anvilContainerId": f"container-{index}"},
                     "measurement": measured})
    return rows


class E3LocalTests(unittest.TestCase):
    def test_complete_four_mode_smoke_has_no_formal_statistics(self):
        rows = local_records()
        result = summarize(rows)
        self.assertTrue(result["localComplete"])
        self.assertFalse(result["formalComplete"])
        self.assertEqual(result["pairedTests"], [])
        self.assertIsNone(result["modes"]["fabric-only"]["gasPerShipment"])
        with self.assertRaisesRegex(ValueError, "local E3 smoke"):
            analyze_formal(rows, allow_partial=True)

    def test_missing_mode_and_workload_mismatch(self):
        rows = local_records()
        self.assertFalse(summarize(rows[:-1])["localComplete"])
        bad = copy.deepcopy(rows)
        bad[-1]["measurement"]["workload_digest"] = "0x" + "f" * 64
        with self.assertRaisesRegex(ValueError, "workload"):
            summarize(bad)

    def test_reused_stack_and_missing_receipt(self):
        rows = local_records()
        bad = copy.deepcopy(rows)
        bad[-1]["ledgerIsolation"]["anvilContainerId"] = bad[0]["ledgerIsolation"]["anvilContainerId"]
        with self.assertRaisesRegex(ValueError, "reused"):
            summarize(bad)
        bad = copy.deepcopy(rows)
        bad[-1]["measurement"]["transactions"] = []
        with self.assertRaisesRegex(ValueError, "receipts"):
            summarize(bad)


if __name__ == "__main__":
    unittest.main()
