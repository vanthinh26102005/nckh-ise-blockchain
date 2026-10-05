#!/usr/bin/env python3
"""Prepare Slurm scripts only. Configuration must come from the actual allocation."""
import argparse, json, re, shlex, subprocess
from pathlib import Path

PHASES = ('gate', 'preflight', 'e1', 'e2', 'e3', 'e4')
REPO = Path(__file__).resolve().parents[1]


def render(config, phase, output_root, revision):
    for field in ('partition', 'account', 'gres', 'time', 'container_runtime', 'compute_host'):
        value = config.get(field)
        if not isinstance(value, str) or not value.strip() or '\n' in value or '\r' in value:
            raise ValueError(f'{field} must come from actual HPC configuration')
    for field in ('partition','account','gres','compute_host'):
        if not re.fullmatch(r'[A-Za-z0-9_.:,/-]+',config[field]): raise ValueError(f'invalid Slurm token: {field}')
    if config['container_runtime'] != 'docker':
        raise ValueError('current Fabric topology requires Docker; no assumed runtime adapter')
    if config.get('concurrency', 1) != 1:
        raise ValueError('fixed topology ports/names require one job at a time per compute host')
    if not re.fullmatch(r'\d+-\d{2}:\d{2}:\d{2}|\d{1,3}:\d{2}:\d{2}', config['time']):
        raise ValueError('time must be Slurm D-HH:MM:SS or HH:MM:SS')
    memory = config.get('memory_gib')
    cpus = config.get('cpus')
    if type(memory) is not int or memory <= 0 or type(cpus) is not int or cpus <= 0:
        raise ValueError('positive integer memory_gib/cpus required')
    commands = config.get('commands', {}).get(phase)
    if not isinstance(commands, list) or not commands or any(not isinstance(c,list) or not c or any(not isinstance(a,str) or '\0' in a for a in c) for c in commands):
        raise ValueError(f'commands.{phase} must contain explicit command argument arrays')
    root = Path(output_root).resolve()
    if root.is_relative_to(REPO): raise ValueError('runtime/output root must be outside Git')
    if not re.fullmatch(r'[0-9a-f]{40}', revision): raise ValueError('full source revision required')
    gate = config.get('gate')
    if phase in ('e3','e4') and (not gate or not Path(gate).is_file()):
        raise ValueError('passed gate path is required before preparing formal E3/E4')
    if gate and phase in ('e3','e4'):
        data=json.loads(Path(gate).read_text())
        if data.get('status')!='passed' or data.get('protocolVersion')!=2 or data.get('verificationKind')!='sp1-groth16' or data.get('gitCommit')!=revision:
            raise ValueError('gate must be real protocol-v2 evidence from this revision')
    header = ['#!/usr/bin/env bash', f'#SBATCH --job-name=eudr-{phase}',
              f'#SBATCH --partition={config["partition"]}', f'#SBATCH --account={config["account"]}',
              f'#SBATCH --gres={config["gres"]}', f'#SBATCH --time={config["time"]}',
              f'#SBATCH --mem={memory}G', f'#SBATCH --cpus-per-task={cpus}',
              'set -euo pipefail', 'umask 077', 'export CARGO_BUILD_JOBS=1 RAYON_NUM_THREADS=2',
              f'export SOURCE_REVISION={revision}',
              ': "${SLURM_JOB_ID:?must run inside a Slurm allocation}"',
              f'test "$(hostname)" = {shlex.quote(config["compute_host"])} || {{ echo "wrong compute host" >&2; exit 2; }}',
              'command -v docker >/dev/null', 'command -v flock >/dev/null',
              'docker info >/dev/null',
              'exec 9>/tmp/eudr-hpc-topology.lock', 'flock -n 9 || { echo "topology already allocated on this host" >&2; exit 2; }',
              'test -z "$(docker ps -aq --filter name=^e2-)" || { echo "existing Fabric topology must be reviewed/collected first" >&2; exit 2; }',
              f'RUN_ROOT={shlex.quote(str(root))}/"${{SLURM_JOB_ID}}-{phase}"',
              'test ! -e "$RUN_ROOT" || { echo "output already exists" >&2; exit 2; }',
              'mkdir -p "$RUN_ROOT/workspace" "$RUN_ROOT/output"',
              f'git -C {shlex.quote(str(REPO))} archive {revision} | tar -x -C "$RUN_ROOT/workspace"',
              'export EUDR_RUN_ROOT="$RUN_ROOT" EUDR_OUTPUT="$RUN_ROOT/output"',
              'cd "$RUN_ROOT/workspace"',
              'trap \'printf "%s\\n" "Collect and review outputs before cleanup: $RUN_ROOT"\' EXIT']
    if config.get('prover') == 'cuda':
        header += ['command -v nvidia-smi >/dev/null', 'nvidia-smi --query-gpu=name,driver_version --format=csv',
                   'command -v nvcc >/dev/null', 'nvcc --version']
    elif config.get('prover') != 'cpu': raise ValueError('prover must be explicit cpu or cuda')
    if gate: header.append(f'export EUDR_GATE={shlex.quote(str(Path(gate).resolve()))}')
    manifest = config.get('policy_manifest')
    if phase != 'e1':
        if not manifest or not Path(manifest).is_file(): raise ValueError('trusted prebuilt policy_manifest required')
        if json.loads(Path(manifest).read_text()).get('protocolVersion') != 2: raise ValueError('policy manifest must be v2')
        header.append(f'export POLICY_MANIFEST={shlex.quote(str(Path(manifest).resolve()))}')
    header += [shlex.join(command) for command in commands]
    return '\n'.join(header)+'\n'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--phase', choices=PHASES, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--runtime-root', type=Path, required=True)
    args=parser.parse_args()
    if args.out.exists(): parser.error('script output already exists')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()
    script=render(json.loads(args.config.read_text()),args.phase,args.runtime_root,revision)
    args.out.parent.mkdir(parents=True,exist_ok=True)
    with args.out.open('x') as stream: stream.write(script)
    print(json.dumps({'operation':'dry-run-script-generation','phase':args.phase,'sourceRevision':revision,
                      'script':str(args.out),'submitted':False}))

if __name__=='__main__': main()
