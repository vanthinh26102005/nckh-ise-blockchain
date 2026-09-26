#!/usr/bin/env bash
# Shared helpers for the real E2 Slurm jobs (issues #26, #27). Source it, do not run it.
# Fabric Raft + Gateway + Anvil + anchor server use fixed ports/containers, so
# only one E2 stack may run on a node at a time.
set -euo pipefail

: "${REPO:?set REPO}" "${OUT:?set OUT}" "${RUN_ROOT:?set RUN_ROOT}"
LOGS="$OUT/logs"
mkdir -p "$LOGS" "$OUT/bin" "$OUT/metadata"
PIDS=()

log() { printf '[%s] %s\n' "$(date -u +%FT%TZ)" "$*"; }

wait_http() {
  local url=$1 name=$2
  for _ in $(seq 1 120); do
    if curl -fsS "$url" >/dev/null 2>&1; then log "$name healthy: $url"; return 0; fi
    sleep 2
  done
  log "ERROR: $name not healthy at $url"
  return 1
}

refuse_if_busy() {
  if docker ps --format '{{.Names}}' | grep -q '^e2-'; then
    log "ERROR: e2-* containers already running on $(hostname); another E2 stack is active. Not touching it."
    docker ps --filter name=e2- --format '{{.Names}} {{.Status}}'
    exit 3
  fi
  local port
  for port in 7050 7051 8050 8080 8545 8546 9050 9051; do
    if (echo >"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      log "ERROR: port $port already in use on $(hostname)"
      exit 3
    fi
  done
}

record_metadata() {
  local meta="$OUT/metadata"
  {
    echo "date_utc=$(date -u +%FT%TZ)"
    echo "host=$(hostname)"
    echo "git_commit=$(git -C "$REPO" rev-parse HEAD)"
    echo "git_dirty_files=$(git -C "$REPO" status --porcelain | wc -l)"
    echo "slurm_job_id=${SLURM_JOB_ID:-}"
    echo "slurm_partition=${SLURM_JOB_PARTITION:-}"
    echo "slurm_nodelist=${SLURM_JOB_NODELIST:-}"
    echo "slurm_cpus_on_node=${SLURM_CPUS_ON_NODE:-}"
    echo "slurm_gpus=${SLURM_JOB_GPUS:-${SLURM_GPUS_ON_NODE:-}}"
    echo "cuda_visible_devices=${CUDA_VISIBLE_DEVICES:-}"
  } > "$meta/run.env"
  git -C "$REPO" status --porcelain > "$meta/git_status.txt"
  scontrol show job "${SLURM_JOB_ID:-0}" > "$meta/slurm_job.txt" 2>&1 || true
  (module list 2>&1 || true) > "$meta/modules.txt"
  nvidia-smi > "$meta/nvidia-smi.txt" 2>&1 || true
  nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv > "$meta/gpu.csv" 2>&1 || true
  (nvcc --version 2>&1 || true) > "$meta/nvcc.txt"
  lscpu > "$meta/lscpu.txt" 2>&1 || true
  free -h > "$meta/mem.txt" 2>&1 || true
  for tool in "rustc --version" "cargo --version" "go version" "node --version" \
      "anvil --version" "docker --version" "docker compose version" "python3 --version"; do
    echo "\$ $tool"; $tool 2>&1 || true
  done > "$meta/toolchain.txt"
}

start_stack() {
  refuse_if_busy
  local raft="$REPO/infra/fabric/raft"
  # bootstrap.sh refuses to overwrite an old runtime; archive it into RUN_ROOT instead of deleting it.
  if [[ -e "$raft/runtime" ]]; then
    log "Archiving previous Fabric runtime to $OUT/old-fabric-runtime"
    mv "$raft/runtime" "$OUT/old-fabric-runtime"
  fi
  log "Starting Fabric Raft (2 org, 2 peer, 3 orderer)"
  bash "$raft/bootstrap.sh" up > "$LOGS/fabric-bootstrap.log" 2>&1

  log "Starting Fabric Gateway"
  (cd "$REPO/infra/fabric/gateway" && go build -o "$OUT/bin/gateway" .) > "$LOGS/gateway-build.log" 2>&1
  FABRIC_CRYPTO_ROOT="$raft/runtime/organizations/peerOrganizations/org1.example.com" \
  FABRIC_CHANNEL=e2channel GATEWAY_LISTEN=127.0.0.1:8080 \
    "$OUT/bin/gateway" > "$LOGS/gateway.log" 2>&1 &
  PIDS+=($!)
  wait_http http://127.0.0.1:8080/health gateway

  log "Starting Anvil"
  anvil --port 8545 > "$LOGS/anvil.log" 2>&1 &
  PIDS+=($!)
  local ok=0
  for _ in $(seq 1 60); do
    if curl -fsS -X POST -H 'content-type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' \
        http://127.0.0.1:8545 >/dev/null 2>&1; then ok=1; break; fi
    sleep 1
  done
  [[ $ok == 1 ]] || { log "ERROR: Anvil not responding"; return 1; }
  log "Anvil healthy: http://127.0.0.1:8545"

  log "Starting anchor server"
  if [[ ! -d "$REPO/contracts/node_modules" ]]; then
    (cd "$REPO/contracts" && npm ci) > "$LOGS/npm.log" 2>&1
  fi
  (cd "$REPO/contracts" && exec node test/anvil_sp1_anchor_server.mjs) > "$LOGS/anchor.log" 2>&1 &
  PIDS+=($!)
  wait_http http://127.0.0.1:8546/health anchor
}

stop_stack() {
  set +e
  log "Stopping background services"
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null; done
  wait 2>/dev/null
  # Only our own compose project (e2raft); never touches other users' containers.
  bash "$REPO/infra/fabric/raft/bootstrap.sh" down >> "$LOGS/fabric-bootstrap.log" 2>&1
  log "Stack stopped"
}

check_tools() {
  local missing=0 tool
  for tool in docker go node npm anvil cargo python3 jq curl nvidia-smi; do
    command -v "$tool" >/dev/null || { log "ERROR: missing tool: $tool"; missing=1; }
  done
  docker compose version >/dev/null 2>&1 || { log "ERROR: docker compose unavailable"; missing=1; }
  docker info >/dev/null 2>&1 || { log "ERROR: cannot talk to Docker daemon"; missing=1; }
  cargo prove --version >/dev/null 2>&1 || { log "ERROR: SP1 toolchain (cargo prove) missing"; missing=1; }
  [[ $missing == 0 ]] || exit 6
}

# Runs a command, recording wall time and peak memory when GNU time exists.
timed() {
  local out=$1 err=$2; shift 2
  if [[ -x /usr/bin/time ]]; then
    /usr/bin/time -v "$@" > "$out" 2> "$err"
  else
    local start=$SECONDS
    "$@" > "$out" 2> "$err"
    echo "wall_seconds=$((SECONDS - start))" >> "$err"
  fi
}

build_runner_cuda() {
  log "Building e2_real_benchmark with --features cuda"
  export CARGO_TARGET_DIR="$RUN_ROOT/cache/cargo-target"
  cargo build --release --manifest-path "$REPO/crates/sp1-e2e/Cargo.toml" -p sp1-e2e \
    --features cuda --bin e2_real_benchmark > "$LOGS/cargo-build.log" 2>&1
  RUNNER="$CARGO_TARGET_DIR/release/e2_real_benchmark"
  test -x "$RUNNER"
  cp "$RUNNER" "$OUT/bin/"
}
