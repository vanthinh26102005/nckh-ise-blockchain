#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$REPO_ROOT"
PYTHON=${PYTHON:-python3}
: "${RUN_ROOT:?Set RUN_ROOT to a writable directory outside the repository}"
RUN_ROOT=$("$PYTHON" - "$RUN_ROOT" "$REPO_ROOT" <<'PY'
from pathlib import Path
import sys
root, repo = map(lambda p: Path(p).expanduser().resolve(), sys.argv[1:])
if root == repo or repo in root.parents:
    raise SystemExit("RUN_ROOT must be outside the Git working tree")
root.mkdir(parents=True, exist_ok=True)
print(root)
PY
)
export RUN_ROOT
E1_EVENTS=${E1_EVENTS:-8,16,32,64}
E1_SEEDS=${E1_SEEDS:-30}
export RAYON_NUM_THREADS=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1
export MPLBACKEND=Agg
RUN_DIR=$(mktemp -d "$RUN_ROOT/e1-${SLURM_JOB_ID:-local}-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
export RUN_DIR E1_EVENTS E1_SEEDS
export MPLCONFIGDIR="$RUN_DIR/matplotlib-cache"
exec > >(tee "$RUN_DIR/run.log") 2>&1
echo "E1 output: $RUN_DIR"
E1_PHASE=preflight
export E1_PHASE
finish() {
    local code=$?
    trap - EXIT
    "$PYTHON" - "$code" <<'PY'
import json, os, sys
from datetime import datetime, timezone
from pathlib import Path
Path(os.environ['RUN_DIR'], 'status.json').write_text(json.dumps({
    'exit_code': int(sys.argv[1]), 'phase': os.environ['E1_PHASE'],
    'completed_at_utc': datetime.now(timezone.utc).isoformat(),
    'passed': int(sys.argv[1]) == 0 and os.environ['E1_PHASE'] == 'complete',
}, indent=2) + '\n')
PY
    echo "E1 finished: phase=$E1_PHASE exit=$code output=$RUN_DIR"
    exit "$code"
}
trap finish EXIT
"$PYTHON" -c 'import numpy, pandas, matplotlib'
git rev-parse HEAD > "$RUN_DIR/git-commit.txt"
git status --short > "$RUN_DIR/git-status.txt"
git diff --binary HEAD > "$RUN_DIR/source.diff"
cp Cargo.lock "$RUN_DIR/Cargo.lock"
if type module >/dev/null 2>&1; then
    module list > "$RUN_DIR/modules.txt" 2>&1
else
    echo "Environment modules are not available on this host." > "$RUN_DIR/modules.txt"
fi
if command -v lscpu >/dev/null 2>&1; then
    lscpu > "$RUN_DIR/cpu.txt"
else
    sysctl hw machdep.cpu > "$RUN_DIR/cpu.txt"
fi
if [[ -n "${SLURM_JOB_ID:-}" ]]; then
    scontrol show job "$SLURM_JOB_ID" > "$RUN_DIR/slurm-allocation.txt"
fi
"$PYTHON" - <<'PY'
import hashlib, json, os, platform, subprocess, tarfile
from datetime import datetime, timezone
from pathlib import Path
out = Path(os.environ['RUN_DIR'])
def command(*args):
    return subprocess.check_output(args, text=True).strip()
files = sorted(set(command('git', 'ls-files', '--cached', '--others', '--exclude-standard').splitlines()))
source_files = [p for p in files if p.endswith(('.rs', '.toml', '.lock', '.py', '.sh', '.sbatch')) or p in ('Makefile', 'rust-toolchain.toml')]
with tarfile.open(out / 'source.tar.gz', 'w:gz') as archive:
    for path in source_files:
        archive.add(path, arcname=path)
metadata = {
    'started_at_utc': datetime.now(timezone.utc).isoformat(),
    'mode': 'slurm-cpu' if os.environ.get('SLURM_JOB_ID') else 'local-cpu',
    'platform': platform.platform(), 'machine': platform.machine(),
    'cpu_logical': os.cpu_count(), 'hostname': platform.node(),
    'rustc_verbose': command('rustc', '-vV'), 'cargo': command('cargo', '--version'),
    'python': platform.python_version(), 'python_packages': command(os.sys.executable, '-m', 'pip', 'freeze'),
    'git_commit': command('git', 'rev-parse', 'HEAD'),
    'working_tree_status': command('git', 'status', '--short'),
    'cargo_lock_sha256': hashlib.sha256(Path('Cargo.lock').read_bytes()).hexdigest(),
    'source_archive_sha256': hashlib.sha256((out / 'source.tar.gz').read_bytes()).hexdigest(),
    'events_per_lot': [int(v) for v in os.environ['E1_EVENTS'].split(',')],
    'seeds_per_cell': int(os.environ['E1_SEEDS']), 'circuits': ['c1', 'c2', 'c3', 'c4', 'c5'],
    'profile': 'coffee-default', 'jobs': 1, 'gpu_requested': False,
    'environment': {k: v for k, v in os.environ.items() if k.startswith('SLURM_') or k in ('RAYON_NUM_THREADS', 'OMP_NUM_THREADS', 'OPENBLAS_NUM_THREADS', 'MKL_NUM_THREADS', 'RUSTFLAGS', 'CARGO_BUILD_TARGET')},
    'cpu_affinity': sorted(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else None,
    'ram_scope': 'Process lifetime peak RSS including setup; not isolated per circuit/seed',
    'warmup': 'Separate process, one seed for each selected events/lot; excluded from raw.csv',
}
(out / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
PY
E1_PHASE=build
cargo build --locked --release -p e1-bench > "$RUN_DIR/build.log" 2>&1
cargo metadata --locked --no-deps --format-version 1 > "$RUN_DIR/cargo-metadata.json"
BIN_DIR=$("$PYTHON" - "$RUN_DIR/cargo-metadata.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))['target_directory'] + '/release')
PY
)
BENCH=("$BIN_DIR/e1-bench" --events "$E1_EVENTS" --circuits c1,c2,c3,c4,c5 --profile coffee-default --jobs 1 --strict-output)
"$PYTHON" - "$BIN_DIR/e1-bench" "$RUN_DIR/binary-sha256.txt" <<'PY'
import hashlib, sys
from pathlib import Path
Path(sys.argv[2]).write_text(hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest() + '\n')
PY
E1_PHASE=warmup
echo "Warm-up: $E1_EVENTS events/lot, one seed, coffee-default"
"${BENCH[@]}" --seeds 1 --out "$RUN_DIR/warmup.csv"
E1_PHASE=benchmark
printf '%q ' "${BENCH[@]}" --seeds "$E1_SEEDS" --out "$RUN_DIR/raw.csv" > "$RUN_DIR/command.txt"
printf '\n' >> "$RUN_DIR/command.txt"
echo "Measured run: $E1_EVENTS events/lot, $E1_SEEDS seeds, C1-C5"
"${BENCH[@]}" --seeds "$E1_SEEDS" --out "$RUN_DIR/raw.csv"
E1_PHASE=analysis
"$PYTHON" scripts/e1_analyze.py --raw "$RUN_DIR/raw.csv" --out-dir "$RUN_DIR" \
    --expected-seeds "$E1_SEEDS" --expected-events "$E1_EVENTS" --base-grid \
    --command "$(cat "$RUN_DIR/command.txt")"
E1_PHASE=complete
