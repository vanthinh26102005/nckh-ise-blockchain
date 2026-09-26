#!/usr/bin/env bash
# Issue #27: formal E2 benchmark (paper RQ2, settlement on Anvil).
# Submit ONLY after the #26 pilot is reviewed and the estimated runtime fits 72h.
# 30 seeds x 60 min = at least 30 h of Poisson arrivals, one sequential process, no job array.
# Submit from the repo:  sbatch infra/slurm/e2-formal.sbatch
#SBATCH --job-name=e2-formal
#SBATCH --nodes=1
#SBATCH --ntasks=1
#SBATCH --cpus-per-task=16
#SBATCH --mem=64G
#SBATCH --partition=defq
#SBATCH --gres=gpu:a100:1
#SBATCH --time=72:00:00
#SBATCH --output=/datastore/uitchain/epcis-zk-rollup-eudr-runs/slurm/%x-%j.out
#SBATCH --error=/datastore/uitchain/epcis-zk-rollup-eudr-runs/slurm/%x-%j.err
set -euo pipefail

export REPO=${REPO:-$HOME/data/epcis-zk-rollup-eudr}
export RUN_ROOT=${RUN_ROOT:-/datastore/$USER/epcis-zk-rollup-eudr-runs}
export OUT="$RUN_ROOT/e2-formal/${SLURM_JOB_ID:-manual}-$(date -u +%Y%m%dT%H%M%SZ)"
module load cuda12.8/toolkit/12.8.1 2>/dev/null || echo "WARN: cuda module not loaded"
source "$REPO/infra/slurm/e2_stack.sh"
trap stop_stack EXIT

record_metadata
nvidia-smi -L || { log "ERROR: no GPU visible"; exit 4; }
check_tools
build_runner_cuda
start_stack

# Real workload: no --accelerated.
CMD=("$RUNNER" --prover cuda --lambda-events-per-min 480 --duration-min 60 --seeds 30
     --run-id "${SLURM_JOB_ID:-$(date +%s)}" --out "$OUT/raw.jsonl")
echo "${CMD[*]}" > "$OUT/metadata/runner_command.txt"
log "Running: ${CMD[*]}"
timed "$LOGS/runner.log" "$LOGS/runner.time.log" "${CMD[@]}"

python3 "$REPO/scripts/e2_real_analyze.py" --raw "$OUT/raw.jsonl" --out "$OUT/summary.json" \
  --expected-seeds 30 > "$LOGS/analyze.log" 2>&1
log "Formal run done. Output: $OUT"
