"""Small regressions for local process cleanup and fresh-ledger campaign boundaries."""
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import e3_campaign
from e4_preflight import mode_order
from local_watchdog import run


class ValidationRegressions(unittest.TestCase):
    def test_watchdog_kills_ignoring_descendant_after_timeout_or_parent_exit(self):
        # Removing unconditional process-group cleanup leaves this real child alive.
        child = """import os, signal, sys, time
from pathlib import Path
signal.signal(signal.SIGTERM, signal.SIG_IGN)
Path(sys.argv[1]).write_text(str(os.getpid()))
time.sleep(60)
"""
        parent = """import subprocess, sys, time
from pathlib import Path
subprocess.Popen([sys.executable, '-c', sys.argv[1], sys.argv[2]])
deadline = time.monotonic() + 5
while not Path(sys.argv[2]).exists():
    if time.monotonic() > deadline: raise SystemExit(3)
    time.sleep(.01)
if sys.argv[3] == 'timeout': time.sleep(60)
"""
        for mode in ('timeout', 'exit'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                pid_path = Path(directory) / 'child.pid'
                pid = None
                try:
                    result = run([sys.executable, '-c', parent, child, str(pid_path), mode], seconds=1)
                    self.assertTrue(pid_path.exists(), 'child must start before the cleanup check')
                    pid = int(pid_path.read_text())
                    self.assertEqual(result['reason'], 'wall_time_limit' if mode == 'timeout' else None)
                    self.assertEqual(result['status'], 'local validation incomplete' if mode == 'timeout' else 'passed')
                    deadline = time.monotonic() + 2
                    while time.monotonic() < deadline:
                        state = subprocess.run(['ps', '-p', str(pid), '-o', 'stat='], capture_output=True, text=True).stdout.strip()
                        if not state or state.startswith('Z'):
                            break
                        time.sleep(.02)
                    else:
                        self.fail('watchdog left a SIGTERM-ignoring descendant running')
                finally:
                    if pid is None and pid_path.exists():
                        pid = int(pid_path.read_text())
                    if pid is not None:
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass

    def test_campaign_commands_bind_order_identity_and_private_workspace(self):
        args = SimpleNamespace(runtime_root=Path('/tmp/eudr-campaign-test'), binary=Path('/tmp/cell'),
                               gate=Path('/tmp/gate.json'), policy_manifest=Path('/tmp/policy.json'),
                               out_dir=Path('/tmp/eudr-raw-test'), prover='cuda', wall_time_s=7200)
        plan = e3_campaign.commands(args, 'a' * 40)
        self.assertEqual(len(plan), 120)
        self.assertEqual(len({row['root'] for row in plan}), 120)
        self.assertEqual(len({row['cell']['ledger_namespace'] for row in plan}), 120)
        for seed in range(30):
            group = plan[seed * 4:seed * 4 + 4]
            self.assertEqual(tuple(row['cell']['mode'] for row in group), mode_order(seed))
            for position, row in enumerate(group):
                argv = row['run_argv']
                value = lambda option: argv[argv.index(option) + 1]
                self.assertEqual(row['cell']['order'], position)
                self.assertEqual(value('--mode'), row['cell']['mode'])
                self.assertEqual(value('--seed'), str(seed))
                self.assertEqual(value('--git-commit'), 'a' * 40)
                self.assertEqual(Path(value('--isolation-manifest')), Path(row['root']) / 'isolation.json')
                self.assertEqual(Path(row['workspace']).parent, Path(row['root']))
                self.assertEqual(row['stack_up_argv'][-1], 'up')
                self.assertEqual(row['stack_down_argv'][-1], 'down')

    def test_campaign_retains_uncollected_ledger_and_tears_down_after_marker(self):
        # Only external npm/stack/benchmark commands are mocked; real paths, main,
        # JSON collection, identity validation and marker writes are exercised.
        for failure in (None, 'run', 'identity', 'marker'):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                base = Path(directory)
                source = base / 'source'
                source.mkdir()
                (source / 'README.md').write_text('test source')
                allocation = base / 'allocation'
                runtime = allocation / 'cells'
                out = base / 'raw'
                revision = 'a' * 40
                cell = dict(seed=0, mode='fabric-only', order=0, ledger_namespace='e3-0-fabric-only')
                down = []

                def external_command(argv, **kwargs):
                    if argv[-1] == 'down':
                        root = Path(kwargs['env']['EUDR_RUN_ROOT'])
                        marker = json.loads((root / 'collected.json').read_text())
                        self.assertTrue(marker['rawCollected'])
                        self.assertEqual(marker['status'], 'censored')
                        down.append(root)
                    elif argv[0] == 'python3':
                        if failure == 'run':
                            raise subprocess.CalledProcessError(2, argv)
                        path = Path(argv[argv.index('--out') + 1])
                        path.write_text(json.dumps(dict(seed=0, mode='proposed' if failure == 'identity' else 'fabric-only',
                                                        git_commit=revision, measurement={'status': 'censored'})))
                    return subprocess.CompletedProcess(argv, 0)

                original_write = Path.write_text

                def write_marker(path, data, *args, **kwargs):
                    if failure == 'marker' and path.name == 'collected.json':
                        raise OSError('simulated collection storage failure')
                    return original_write(path, data, *args, **kwargs)

                argv = ['e3_campaign.py', '--runtime-root', str(runtime), '--binary', str(base / 'binary'),
                        '--gate', str(base / 'gate.json'), '--policy-manifest', str(base / 'policy.json'),
                        '--out-dir', str(out), '--prover', 'cuda', '--execute']
                with patch.object(e3_campaign, 'REPO', source), patch.object(e3_campaign, 'rows', return_value=[cell]), \
                     patch.object(e3_campaign.subprocess, 'run', side_effect=external_command), \
                     patch.object(Path, 'write_text', write_marker), patch.object(sys, 'argv', argv), \
                     patch.dict(os.environ, SOURCE_REVISION=revision, SLURM_JOB_ID='1', EUDR_RUN_ROOT=str(allocation)), \
                     contextlib.redirect_stdout(io.StringIO()):
                    if failure:
                        with self.assertRaises((subprocess.CalledProcessError, RuntimeError, OSError)):
                            e3_campaign.main()
                    else:
                        e3_campaign.main()
                self.assertEqual(len(down), 0 if failure else 1)
                root = runtime / 'seed-0-fabric-only'
                isolation = json.loads((root / 'isolation.json').read_text())
                self.assertEqual(isolation['sourceRevision'], revision)
                self.assertEqual(isolation['seed'], 0)
                self.assertEqual(isolation['mode'], 'fabric-only')
                self.assertTrue(isolation['freshLedger'])
                self.assertEqual(Path(isolation['runtimePath']), root.resolve())


if __name__ == '__main__':
    unittest.main()
