#!/usr/bin/env bash
# Issue #26: E2 CUDA/Fabric/Anvil pilot. 1 seed, 1 minute, --accelerated. NOT paper evidence.
# Submit from the repo:  sbatch infra/slurm/e2-pilot.sbatch
# The log directory must exist before submitting: mkdir -p $RUN_ROOT/slurm
#SBATCH --job-name=e2-pilot
#SBATCH --nodes=1
#SBATCH --ntasks=1
#SBATCH --cpus-per-task=16
#SBATCH --mem=64G
#SBATCH --partition=defq
#SBATCH --gres=gpu:a100:1
#SBATCH --time=04:00:00
#SBATCH --output=/datastore/uitchain/epcis-zk-rollup-eudr-runs/slurm/%x-%j.out
#SBATCH --error=/datastore/uitchain/epcis-zk-rollup-eudr-runs/slurm/%x-%j.err
set -euo pipefail

export REPO=${REPO:-$HOME/data/epcis-zk-rollup-eudr}
export RUN_ROOT=${RUN_ROOT:-/datastore/$USER/epcis-zk-rollup-eudr-runs}
export OUT="$RUN_ROOT/e2-pilot/${SLURM_JOB_ID:-manual}-$(date -u +%Y%m%dT%H%M%SZ)"
module load cuda12.8/toolkit/12.8.1 2>/dev/null || echo "WARN: cuda module not loaded"
source "$REPO/infra/slurm/e2_stack.sh"
trap stop_stack EXIT

record_metadata
nvidia-smi -L || { log "ERROR: no GPU visible"; exit 4; }
check_tools
build_runner_cuda
start_stack

CMD=("$RUNNER" --prover cuda --accelerated --duration-min 1 --seeds 1
     --run-id "${SLURM_JOB_ID:-$(date +%s)}" --out "$OUT/raw.jsonl")
echo "${CMD[*]}" > "$OUT/metadata/runner_command.txt"
log "Running: ${CMD[*]}"
timed "$LOGS/runner.log" "$LOGS/runner.time.log" "${CMD[@]}"

test -s "$OUT/raw.jsonl" || { log "ERROR: raw.jsonl is empty"; exit 5; }
# Acceptance for #26: every lot has a proof and an Anvil receipt; report proof time.
python3 - "$OUT/raw.jsonl" > "$OUT/pilot_check.txt" <<'PY'
import json, sys, statistics as st
lots = [r for r in map(json.loads, open(sys.argv[1])) if r.get("kind") == "lot"]
if not lots or any(not l.get("transactionHash") or l.get("status") != "ok" for l in lots):
    sys.exit("FAIL: missing lot or Anvil receipt")
proof = [l["proofMs"] for l in lots]
events = [l["eventCount"] for l in lots]
print(f"lots={len(lots)} events_per_lot_median={st.median(events)}")
print(f"proofMs median={st.median(proof)} max={max(proof)}")
print(f"anvilAnchorMs median={st.median(l['anvilAnchorMs'] for l in lots)}")
print("tx example:", lots[0]["transactionHash"])
PY
cat "$OUT/pilot_check.txt"
python3 "$REPO/scripts/e2_real_analyze.py" --raw "$OUT/raw.jsonl" --out "$OUT/summary.json" \
  --expected-seeds 1 --allow-accelerated > "$LOGS/analyze.log" 2>&1 \
  || log "WARN: analyzer failed, see $LOGS/analyze.log"
log "Pilot done. Output: $OUT"
