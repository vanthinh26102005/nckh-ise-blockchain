#!/usr/bin/env bash
# Allocation-local topology lifecycle. Run serially and collect output before down.
set -euo pipefail
: "${EUDR_RUN_ROOT:?allocation workspace/runtime root required}"
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
case "$REPO/" in "$EUDR_RUN_ROOT"/*) ;; *) echo 'stack must run from its private allocation workspace' >&2; exit 2;; esac
OWNER="$EUDR_RUN_ROOT/stack-owner"
compose=(docker compose -p e2raft -f "$REPO/infra/fabric/raft/compose.yaml")
case "${1:-}" in
  up)
    : "${POLICY_MANIFEST:?trusted prebuilt registry/program configuration required}"
    : "${ANVIL_IMAGE:?configure an installed Foundry image for this HPC host}"
    test ! -e "$OWNER" || { echo 'stack output exists; collect/review before reset' >&2; exit 2; }
    command -v docker >/dev/null; command -v go >/dev/null; command -v node >/dev/null
    docker image inspect "$ANVIL_IMAGE" >/dev/null
    test -z "$(docker ps -aq --filter name=^e2-)" || { echo 'existing topology needs review' >&2; exit 2; }
    python3 - <<'PY'
import socket
for port in (7050,7051,7053,8050,8053,9050,9053,9051,8080,8545,8546):
    with socket.socket() as sock: sock.bind(('127.0.0.1',port))
PY
    mkdir -p "$EUDR_RUN_ROOT/output"
    printf '%s\n' "$REPO" > "$OWNER"
    bash "$REPO/infra/fabric/raft/bootstrap.sh" > "$EUDR_RUN_ROOT/output/fabric-bootstrap.log" 2>&1
    (cd "$REPO/infra/fabric/gateway"; go build -o "$EUDR_RUN_ROOT/gateway" .)
    FABRIC_CRYPTO_ROOT="$REPO/infra/fabric/raft/runtime/organizations/peerOrganizations/org1.example.com" \
      FABRIC_CHANNEL=e2channel GATEWAY_LISTEN=127.0.0.1:8080 "$EUDR_RUN_ROOT/gateway" \
      > "$EUDR_RUN_ROOT/output/gateway.log" 2>&1 &
    echo "$!" > "$EUDR_RUN_ROOT/gateway.pid"
    docker run -d --name e2-anvil -p 127.0.0.1:8545:8545 --entrypoint anvil "$ANVIL_IMAGE" --host 0.0.0.0 >/dev/null
    node "$REPO/contracts/test/anvil_sp1_anchor_server.mjs" > "$EUDR_RUN_ROOT/output/anchor-server.log" 2>&1 &
    echo "$!" > "$EUDR_RUN_ROOT/anchor.pid"
    python3 - <<'PY'
import json,time,urllib.request
for url in ('http://127.0.0.1:8080/health','http://127.0.0.1:8546/health'):
    for attempt in range(120):
        try:
            with urllib.request.urlopen(url,timeout=2) as response:
                if response.status==200:break
        except OSError:pass
        time.sleep(.5)
    else:raise SystemExit('service health deadline exceeded: '+url)
PY
    ;;
  down)
    test -f "$OWNER" && test "$(cat "$OWNER")" = "$REPO" || { echo 'no ownership marker for this allocation' >&2; exit 2; }
    # PIDs are allocation-owned; outputs and witness archives remain available for collection.
    for file in gateway.pid anchor.pid; do
      if test -f "$EUDR_RUN_ROOT/$file"; then kill "$(cat "$EUDR_RUN_ROOT/$file")" 2>/dev/null || true; fi
    done
    docker rm -f e2-anvil >/dev/null 2>&1 || true
    "${compose[@]}" down --volumes
    rm "$OWNER"
    ;;
  *) echo 'usage: e2_stack.sh up|down (inside private allocation workspace)' >&2; exit 2;;
esac
