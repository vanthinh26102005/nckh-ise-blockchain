#!/usr/bin/env python3
"""Reviewable phase plans; execute only inside the private workspace of a Slurm job."""
import argparse
import hashlib
import json
import os
import re
import socket
import subprocess
from datetime import datetime, timezone
from pathlib import Path

from e1_plan import campaign
from e4_plan import PHASES, generate_rows
from e4_preflight import mode_order, validate_preflight

REPO = Path(__file__).resolve().parents[1]
BIN = 'crates/sp1-e2e/target/release/'


def step(argv, stdout=None):
    return {'argv': list(map(str, argv)), 'stdout': str(stdout) if stdout else None}


def validate_inputs(c, phase, revision):
    required = {
        'gate': (), 'e1': (), 'prepare': ('gate', 'policy_manifest'),
        'preflight': ('gate', 'policy_manifest'), 'e2-pilot': ('gate',),
        'e2': ('gate', 'pilot_report'), 'e3': ('gate', 'policy_manifest'),
        'e4': ('gate', 'policy_manifest', 'preflight'),
    }[phase]
    if phase != 'e1' and (not isinstance(c.get('anvil_image'), str)
                          or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._:@/-]+', c['anvil_image'])):
        raise ValueError('anvil_image must be an explicit prepared Docker image reference')
    if phase in ('preflight', 'e2-pilot', 'e2', 'e3', 'e4') and c.get('prover') != 'cuda':
        raise ValueError('HPC measurement phases require explicit CUDA, not a CPU default')
    if c.get('prover') == 'cuda' and phase != 'e1' and not c.get('gres'):
        raise ValueError('CUDA job requires verified GPU GRES')
    for key in required:
        value = c.get(key)
        if not value or not Path(value).is_absolute() or not Path(value).is_file():
            raise ValueError(f'{phase} requires an existing absolute {key} file')
        data = json.loads(Path(value).read_text())
        if data.get('protocolVersion') != 2:
            raise ValueError(f'{key} is not policy v2')
    if 'gate' in required:
        gate = json.loads(Path(c['gate']).read_text())
        if (gate.get('gitCommit') != revision or gate.get('status') != 'passed'
                or gate.get('verificationKind') != 'sp1-groth16'
                or gate.get('anvilReceiptStatus') != 1 or not gate.get('registry')
                or gate.get('tamperedChildRejected') is not True
                or gate.get('tamperedAggregateProofRejected') is not True
                or gate.get('tamperedAggregateValuesRejected') is not True):
            raise ValueError('a matching real proof/receipt/tamper gate is required')
        if 'policy_manifest' in required:
            policy = json.loads(Path(c['policy_manifest']).read_text())
            if (policy.get('leafVKeyDigest') != gate.get('policyProgramVKeyDigest')
                    or policy.get('aggregateVKeyDigest') != gate.get('aggregateProgramVKeyDigest')
                    or policy.get('aggregateProgramVKey') != gate.get('aggregateProgramVKey')):
                raise ValueError('catalog program keys differ from the reviewed gate')
    if 'preflight' in required:
        value = json.loads(Path(c['preflight']).read_text())
        validate_preflight(value)
        digest = hashlib.sha256(Path(c['policy_manifest']).read_bytes()).hexdigest()
        if (value.get('gitCommit') != revision or value.get('computeHost') != c['compute_host']
                or value.get('registry_manifest_sha256') != digest):
            raise ValueError('preflight source/host/catalog differs from the requested job')
    if 'pilot_report' in required:
        value = json.loads(Path(c['pilot_report']).read_text())
        run = value.get('run', {})
        if (c.get('pilot_reviewed') is not True or value.get('run', {}).get('sourceRevision') != revision
                or value.get('run', {}).get('computeHost') != c['compute_host']
                or run.get('prover') != 'cuda' or run.get('sourceDirty') is not False
                or run.get('mode') != 'accelerated-pilot' or run.get('durationMin') != 1
                or run.get('seeds') != 1 or run.get('lambdaEventsPerMin') != 480
                or value.get('aggregate', {}).get('anchoredLots', 0) < 1):
            raise ValueError('review a real same-source/host GPU pilot before the formal E2 job')
    for key, digest in c.get('input_sha256', {}).items():
        if hashlib.sha256(Path(c[key]).read_bytes()).hexdigest() != digest:
            raise ValueError(f'{key} changed after job generation; regenerate and review')


def plan(c, phase, out, revision):
    """Emit executable argv (never submit or create proofs during planning)."""
    if not re.fullmatch('[0-9a-f]{40}', revision):
        raise ValueError('full committed revision required')
    prover = c['prover']
    setup, prepare, measure = [], [], []
    policy = c.get('policy_manifest')
    epoch = c.get('epoch_base')
    if type(epoch) is not int or not 1 <= epoch < 2**32:
        raise ValueError('epoch_base must be a reviewed positive uint32')
    wall = c.get('wall_time_s')
    if type(wall) is not int or wall <= 3600:
        raise ValueError('wall_time_s must exceed the E3 formal one-hour producer window')
    if phase == 'e1':
        mode, depth = c.get('e1_mode'), c.get('credential_depth')
        if mode not in ('base', 'wrapper') or depth not in (6, 16):
            raise ValueError('choose E1 base/wrapper and credential depth 6/16')
        cell = next(row for row in campaign(out / 'e1')['cells']
                    if row['mode'] == mode and row['credential_depth'] == depth)
        # Pin lockfiles; built-in campaign already includes the production security parameters.
        run = cell['run_argv']
        run.insert(2, '--locked')
        measure = [step(run), step(cell['analyze_argv'])]
        return dict(setup=setup, prepare=prepare, measure=measure, policy_manifest=None, stack=False)
    setup = [step(['cargo', 'build', '--locked', '--release', '--manifest-path', 'crates/sp1-e2e/Cargo.toml',
                   '-p', 'sp1-e2e', '--bin', 'e3_epoch_gate', '--bin', 'e4_real_cell', '--bin', 'e2_real_benchmark']
                  + (['--features', 'cuda'] if prover == 'cuda' else [])),
             step(['npm', 'ci', '--prefix', 'contracts'])]
    gate = BIN + 'e3_epoch_gate'
    cell = BIN + 'e4_real_cell'
    if phase == 'gate':
        policy = str(out / 'gate-policy.json')
        prepare = [step([gate, '--prepare-only', '--prover', prover, '--epoch-id', epoch,
                         '--events-per-leaf', 8, '--out', policy])]
        measure = [step([gate, '--prover', prover, '--epoch-id', epoch, '--events-per-leaf', 8,
                         '--out', out / 'gate-proof.json']),
                   step(['node', 'contracts/test/anvil_e3_smoke.mjs'], out / 'gate-receipt.json'),
                   step(['python3', 'scripts/e3_gate_manifest.py', '--fixture', out / 'gate-proof.json',
                         '--receipt', out / 'gate-receipt.json', '--git-commit', revision,
                         '--out', out / 'gate-manifest.json'])]
    elif phase == 'prepare':
        # Prepare every snapshot before measurement. One catalog is retained for A/B and E4 shards.
        snapshots = [policy]
        for events in (8, 64):
            dest = out / f'ab-{events}-policy.json'
            prepare.append(step([gate, '--prepare-only', '--prover', prover,
                                 '--epoch-id', epoch + (events == 64), '--events-per-leaf', events, '--out', dest]))
            snapshots.append(dest)
        workloads = [dict(seed=s, np=125, shipment=64, epoch_s=60, epochs=60) for s in range(30)]
        workloads += [dict(seed=r['seed'], np=r['np'], shipment=r['shipment'], epoch_s=r['epoch_s'], epochs=4)
                      for r in generate_rows(PHASES)]
        for index, axes in enumerate(workloads):
            dest = out / f'workload-{index}-policy.json'
            args = [cell, '--prover', prover, '--prepare-policy-manifest', dest]
            for key, value in axes.items():
                args += ['--' + key.replace('_', '-'), value]
            prepare.append(step(args))
            snapshots.append(dest)
        policy = str(out / 'catalog.json')
        prepare += [step(['python3', 'scripts/policy_manifest.py', '--out', policy, *snapshots]),
                    step(['python3', 'scripts/e4_plan.py', '--phase', 'all', '--out', out / 'e4-plan.csv',
                          '--summary-out', out / 'e4-plan-summary.json']),
                    step(['python3', 'scripts/e3_plan.py', '--out', out / 'e3-plan.csv'])]
    elif phase == 'preflight':
        measure = [step(['python3', 'scripts/e4_preflight.py', '--binary', gate, '--raw-dir', out / 'ab-raw',
                         '--out', out / 'ab-preflight.json', '--epoch-base', epoch, '--git-commit', revision,
                         '--policy-manifest', policy])]
    elif phase in ('e2-pilot', 'e2'):
        binary = BIN + 'e2_real_benchmark'
        policy = str(out / 'e2-policy.json')  # Never merge away E2 runId/workloadSeeds.
        duration, seeds = (1, 1) if phase == 'e2-pilot' else (60, 30)
        axes = ['--prover', prover, '--lambda-events-per-min', 480, '--duration-min', duration,
                '--seeds', seeds, '--run-id', epoch]
        prepare = [step([binary, *axes, '--prepare-policy-manifest', policy])]
        measure = [step(['python3', 'scripts/e2_preflight.py', '--out', out / 'e2-preflight.json',
                         '--source-revision', revision]),
                   step([binary, *axes, '--preflight', out / 'e2-preflight.json', '--out', out / 'e2-raw.jsonl']
                        + (['--accelerated'] if phase == 'e2-pilot' else [])),
                   step(['python3', 'scripts/e2_real_analyze.py', '--raw', out / 'e2-raw.jsonl',
                         '--policy-manifest', policy, '--out', out / 'e2-analysis.json', '--expected-seeds', seeds]
                        + (['--allow-partial'] if phase == 'e2-pilot' else []))]
    elif phase == 'e3':
        seed, mode = c.get('e3_seed'), c.get('e3_mode')
        if type(seed) is not int or not 0 <= seed < 30 or mode not in mode_order(seed):
            raise ValueError('choose one planned E3 seed 0..29 and a valid mode')
        measure = [step(['python3', 'scripts/e3_run.py', '--binary', cell, '--gate', c['gate'],
                         '--policy-manifest', policy, '--mode', mode, '--seed', seed, '--prover', prover,
                         '--wall-time-s', wall, '--git-commit', revision,
                         '--isolation-manifest', out / 'isolation.json', '--out', out / 'e3-cell.json'])]
    elif phase == 'e4':
        phase4, index = c.get('e4_phase'), c.get('e4_plan_index')
        if phase4 not in PHASES or type(index) is not int or not 0 <= index < (80 if phase4 == 'screening' else 270):
            raise ValueError('choose one planned E4 phase/index (80 screening / 270 confirmation)')
        prepare = [step(['python3', 'scripts/e4_plan.py', '--phase', 'all', '--out', out / 'e4-plan.csv'])]
        measure = [step(['python3', 'scripts/e4_run.py', '--plan', out / 'e4-plan.csv', '--phase', phase4,
                         '--plan-index', index, '--executor', 'real-e3', '--real-binary', cell, '--prover', prover,
                         '--e3-gate-manifest', c['gate'], '--preflight-ab', c['preflight'],
                         '--policy-manifest', policy, '--wall-time-s', wall, '--git-commit', revision,
                         '--created-at', datetime.now(timezone.utc).isoformat(), '--command', 'hpc_run.py e4 single-cell',
                         '--out', out / 'raw.jsonl', '--manifest-out', out / 'manifest.json'])]
    else:
        raise ValueError('unknown HPC phase')
    if phase != 'prepare':
        measure.insert(0, step(['python3', 'scripts/e3_local_warmup.py', '--out', out / 'fabric-warmup.json']))
    return dict(setup=setup, prepare=prepare, measure=measure, policy_manifest=policy, stack=phase != 'prepare')


def run_step(value, env):
    print(json.dumps({'executing': value['argv'], 'stdout': value['stdout']}), flush=True)
    if value['stdout']:
        with Path(value['stdout']).open('x') as output:
            subprocess.run(value['argv'], cwd=REPO, env=env, stdout=output, check=True)
    else:
        subprocess.run(value['argv'], cwd=REPO, env=env, check=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--config', type=Path, required=True)
    p.add_argument('--phase', choices=('gate', 'prepare', 'preflight', 'e1', 'e2-pilot', 'e2', 'e3', 'e4'), required=True)
    p.add_argument('--execute', action='store_true')
    a = p.parse_args()
    c = json.loads(a.config.read_text())
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
    out = Path(os.environ.get('EUDR_OUTPUT', '/outside-git/review-plan'))
    value = plan(c, a.phase, out, revision)
    if not a.execute:
        print(json.dumps({'phase': a.phase, 'sourceRevision': revision, 'submitted': False,
                          'operation': 'dry-run-command-plan', **value}, indent=2))
        return
    runtime = Path(os.environ.get('EUDR_RUN_ROOT', '/missing')).resolve()
    if (not os.environ.get('SLURM_JOB_ID') or not REPO.is_relative_to(runtime)
            or not out.is_absolute() or not out.resolve().is_relative_to(runtime)
            or out.resolve().is_relative_to(REPO) or socket.gethostname() != c['compute_host']):
        p.error('execution requires the generated, private, same-host Slurm workspace/output')
    if os.environ.get('SOURCE_REVISION') != revision:
        p.error('allocation source differs from pinned SOURCE_REVISION')
    if subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=all'], cwd=REPO, text=True).strip():
        p.error('execute only from a clean committed checkout')
    validate_inputs(c, a.phase, revision)
    with (out / 'command-plan.json').open('x') as stream:
        json.dump(value, stream, indent=2)
    env = dict(os.environ, SOURCE_REVISION=revision)
    success = False
    try:
        for command in value['setup'] + value['prepare']:
            run_step(command, env)
        if value['stack']:
            env.update(POLICY_MANIFEST=value['policy_manifest'], ANVIL_IMAGE=c['anvil_image'])
            run_step(step(['docker', 'image', 'inspect', c['anvil_image']], out / 'anvil-image.json'), env)
            run_step(step(['bash', 'scripts/e2_stack.sh', 'up']), env)
        if a.phase == 'gate':
            env['E3_FIXTURE'] = str(out / 'gate-proof.json')
        if a.phase == 'e3':
            record = dict(protocolVersion=2, sourceRevision=revision, computeHost=socket.gethostname(),
                          freshLedger=True, runtimePath=str(runtime), seed=c['e3_seed'], mode=c['e3_mode'],
                          namespace=f'e3-{c["e3_seed"]}-{c["e3_mode"]}', order=mode_order(c['e3_seed']).index(c['e3_mode']))
            with (out / 'isolation.json').open('x') as stream:
                json.dump(record, stream)
        for command in value['measure']:
            run_step(command, env)
        if a.phase == 'e2-pilot':
            report = json.loads((out / 'e2-analysis.json').read_text())
            if report['aggregate']['anchoredLots'] < 1:
                raise RuntimeError('GPU pilot produced no anchored lot; not ready for formal E2')
        success = True
    finally:
        # Keep failed/uncollected ledgers for recovery. Remove only a successfully collected owned stack.
        if success and value['stack']:
            run_step(step(['bash', 'scripts/e2_stack.sh', 'down']), env)
        with (out / 'job-result.json').open('x') as stream:
            json.dump(dict(phase=a.phase, sourceRevision=revision, computeHost=socket.gethostname(),
                           executionComplete=success, formalComplete=False,
                           note='Review raw receipts/accounting and full campaign analyzers separately.'), stream, indent=2)


if __name__ == '__main__':
    main()
