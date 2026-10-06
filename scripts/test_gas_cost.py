import json
import subprocess
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('gas_cost.py')


class GasCostTests(unittest.TestCase):
    def test_decimal_scenarios_and_estimate_label(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '--gas-used', '21000', '--gas-gwei-low', '1', '--gas-gwei-mid', '10', '--gas-gwei-high', '100', '--eth-usd', '2000', '--date', '2026-10-05', '--source', 'fixture assumptions'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual([v['usd'] for v in report['scenarios']], ['0.042', '0.42', '4.2'])
        self.assertIn('estimate', report['kind'])
        self.assertIn('not actual paid mainnet', report['scope'])

    def test_invalid_assumptions_are_rejected(self):
        for value in ['0', '-1', 'NaN', 'Infinity']:
            result = subprocess.run([sys.executable, str(SCRIPT), '--gas-used', '21000', '--gas-gwei-low', value, '--gas-gwei-mid', '10', '--gas-gwei-high', '100', '--eth-usd', '2000', '--date', '2026-10-05', '--source', 'fixture'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
