import tempfile
import unittest
from pathlib import Path

import e1_analyze as e1
import e1_plan


class E1EvidenceTests(unittest.TestCase):
    def test_strict_rejects_stale_versions_and_wrong_hash_scope(self):
        row = {'circuit':'c2_poseidon2_merkle','events_per_lot':8,'seed':1,'credential_depth':6,'status':'ok',
               'prove_time_s':1,'verify_time_ms':1,'proof_size_bytes':100,'peak_ram_gb':1,
               'circuit_version':'v5-plonky3-poseidon2-merkle-selectable-depth',
               'semantic_scope':'Path B circuit-cost research; C2 membership, C4 actor preimage, C5 32-bit Poseidon2 index; not equivalent to Path A SHA-256/256-bit nullifiers',
               'hash':'Poseidon2/Goldilocks (one-field statement digests)',
               'fri_log_blowup':3,'fri_num_queries':33,'fri_query_pow_bits':16,
               'security_model':'approximately 115 conjectured bits (3*33+16) checklist model; not a security certification'}
        e1.validate_grid(e1.pd.DataFrame([row]),1,[row['circuit']],[8],[6],strict=True)
        for key,value in [('circuit_version','v1-legacy'),('semantic_scope','production SHA-256 policy'),('hash','SHA256'),('security_model','certified 128 bits')]:
            changed=dict(row);changed[key]=value
            with self.assertRaises(SystemExit):
                e1.validate_grid(e1.pd.DataFrame([changed]),1,[row['circuit']],[8],[6],strict=True)

    def test_plan_is_external_and_keeps_campaign_coverage(self):
        plan = e1_plan.campaign('/tmp/e1-test-campaign')
        self.assertEqual(len(plan['cells']), 4)
        self.assertEqual([cell['seeds'] for cell in plan['cells']], [30, 10, 30, 10])
        self.assertEqual([cell['events'] for cell in plan['cells']], [[8,16,32,64],[8,64],[8,16,32,64],[8,64]])
        self.assertEqual(plan['sp1_campaign']['samples'], 20)
        self.assertEqual(plan['sp1_campaign']['lots_per_aggregate'], 2)
        with self.assertRaises(ValueError):
            e1_plan.campaign(Path(__file__).resolve().parent.parent / 'results')

    def test_read_preserves_failed_row_and_provenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            raw = Path(tmp) / 'raw.csv'
            raw.write_text('circuit,events_per_lot,seed,prove_ms,verify_ms,proof_bytes,peak_rss_mb,status,note,credential_depth\nc2,8,1,1000,2,300,1024,ok,,6\nwrapper,8,1,,,,,error,upstream failure,6\n')
            frame, _ = e1.read_raw(raw)
            self.assertEqual(len(frame), 2)
            self.assertEqual(frame.iloc[1]['note'], 'upstream failure')
            self.assertTrue(e1.pd.isna(frame.iloc[1]['proof_size_bytes']))

    def test_duplicate_seeds_do_not_count_as_complete(self):
        frame = e1.pd.DataFrame({'circuit': ['c2', 'c2'], 'events_per_lot': [8, 8], 'seed': [1, 1], 'credential_depth': [6, 6], 'status': ['ok', 'ok']})
        with self.assertRaises(SystemExit):
            e1.validate_grid(frame, 1)

    def test_missing_depth_cell_is_rejected(self):
        frame = e1.pd.DataFrame({'circuit': ['c2'], 'events_per_lot': [8], 'seed': [1], 'credential_depth': [6], 'status': ['ok']})
        with self.assertRaises(SystemExit):
            e1.validate_grid(frame, 1, ['c2'], [8], [6, 16])

    def test_missing_metric_is_not_zero_or_a_valid_strict_row(self):
        frame = e1.pd.DataFrame({'circuit': ['c2'], 'events_per_lot': [8], 'seed': [1], 'credential_depth': [6], 'status': ['ok'], 'prove_time_s': [None], 'verify_time_ms': [1], 'proof_size_bytes': [100], 'peak_ram_gb': [1]})
        with self.assertRaises(SystemExit):
            e1.validate_grid(frame, 1)


if __name__ == '__main__':
    unittest.main()
