"""Readiness checks reject final-paper claims without running proofs or builds."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from check_paper_evidence import TASK_IDS, validate

ROOT = Path(__file__).resolve().parents[1]


class PaperEvidenceTests(unittest.TestCase):
    def checklist(self):
        return json.loads((ROOT / 'results/policy-v2/checklist.json').read_text())

    def test_readiness_snapshot_covers_exactly_twenty_tasks(self):
        data = self.checklist()
        validate(data, r'\finalfalse')
        self.assertEqual(len(data['tasks']), 20)
        self.assertEqual({task['id'] for task in data['tasks']}, set(TASK_IDS))
        self.assertTrue(all(task['formalComplete'] is False for task in data['tasks']))

    def test_missing_duplicate_and_unknown_task_ids_are_rejected(self):
        original = self.checklist()
        for mutate in (lambda d: d['tasks'].pop(),
                       lambda d: d['tasks'].append(copy.deepcopy(d['tasks'][0])),
                       lambda d: d['tasks'][0].update(id='T-UNKNOWN-01')):
            data = copy.deepcopy(original)
            mutate(data)
            with self.assertRaisesRegex(ValueError, '20|task IDs'):
                validate(data, r'\finalfalse')

    def test_formal_and_final_completion_claims_are_rejected(self):
        original = self.checklist()
        mutations = (lambda d: d.update(formalComplete=True),
                     lambda d: d.update(finaltrue=True),
                     lambda d: d.update(formalMeasuredRuns=1),
                     lambda d: d['tasks'][0].update(formalComplete=True),
                     lambda d: d['tasks'][0].update(status='completed'),
                     lambda d: d['tasks'][0].update(evidenceClass='formal'),
                     lambda d: d.update(extra={'formal': True}))
        for mutate in mutations:
            data = copy.deepcopy(original)
            mutate(data)
            with self.assertRaises(ValueError):
                validate(data, r'\finalfalse')
        with self.assertRaisesRegex(ValueError, 'finaltrue'):
            validate(original, '\\finaltrue\n')
        validate(original, '% set \\finaltrue only after completion\n\\finalfalse\n')

    def test_cli_reports_readiness_only_and_fails_on_final_paper(self):
        with tempfile.TemporaryDirectory() as directory:
            paper = Path(directory) / 'main.tex'
            paper.write_text('\\finalfalse\n')
            command = [sys.executable, str(ROOT / 'scripts/check_paper_evidence.py'),
                       '--checklist', str(ROOT / 'results/policy-v2/checklist.json'), '--paper', str(paper)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report['evidenceClass'], 'readiness-only')
            self.assertFalse(report['formalComplete'])
            paper.write_text('\\finaltrue\n')
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)


if __name__ == '__main__':
    unittest.main()
