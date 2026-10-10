# Policy-v2 local validation and HPC runbook

This is a preparation guide for the approved [policy-v2 plan](superpowers/plans/2026-10-05-policy-v2-hpc-readiness.md). The authoritative twenty paper tasks are in [todo-list.tex](../EPCIS_ACIIDS_2026_ThinhTriNhan/todo-list.tex). The [checklist](../results/policy-v2/checklist.json) records implementation/readiness, with formal completion disabled. No command below is evidence that an allocation, proof or measurement has already succeeded.

## Source and measurement boundary

Canonical events stay 86 bytes. Private witness and epoch wire formats use version 2. Leaf and aggregate public ABI values are exactly 352 bytes; the aggregate appends `certificateRoot` and `polygonCommitment` at words 9 and 10. Children explicitly distinguish leaf and aggregate. Every child shares its root pair, policy roots bind into Gamma, and owner-approved registry pairs govern all proof anchors. Role 4, threshold 900, 8–64 events per leaf and root advancement are enforced.

Path A uses SHA-256, Ed25519 and a 256-bit sparse nullifier index. Path B remains circuit-cost research with Poseidon2/preimage authorization and its stated depth-32 nullifier scope. FRI 3/33/16 gives the checklist's approximately 115 conjectured-bit model; it is not a security certification or evidence that the two paths prove identical statements.

Formal source identity requires a clean, reviewed commit and matching rebuilt binaries, manifests, gate, preflight and raw records. `git rev-parse HEAD` in a dirty checkout identifies the base commit only. The host build embeds `unknown-dirty-source` when tracked or untracked build inputs differ from the committed source; archived allocation workspaces use explicitly configured `SOURCE_REVISION`. Preserve source and lockfile hashes, full argv, workload fingerprints, registry-manifest hash, program keys, hardware/software records and raw receipts. Historical proofs/program keys/results remain historical after these changes.

Read-only source checks:

```sh
git status --short
git rev-parse HEAD
git diff --check
python3 scripts/check_paper_evidence.py
python3 -m unittest discover -s scripts -p 'test_check_paper_evidence.py'
python3 -m unittest discover -s scripts -p 'test_e3*.py'
node --check contracts/test/anvil_sp1_anchor_server.mjs
node --check contracts/test/five_epoch_smoke.mjs
```

These checks do not run proofs, establish live HPC capacity, or close a formal paper task. Broader Python tests may require their existing analysis dependencies. Commands that compile Rust or Solidity are outside this documentation-only verification step.

## Local resource limits

Set `CARGO_BUILD_JOBS=1` and `RAYON_NUM_THREADS=2` for any separately authorized local build/check. The only permitted guest smoke is execute-only, exactly eight events, with 50 million cycles, 120 seconds, 4 GiB process-tree RSS and 512 MiB swap-growth ceilings. Use an already-built binary and fresh reports outside Git:

```sh
python3 scripts/local_watchdog.py --report /outside/git/new-execute-budget.json -- \
  crates/sp1-e2e/target/debug/local_execute /outside/git/private-witness-v2.bin
```

`local_execute` compares guest output with host policy output and sets `proofGenerated=false`. The latest eight-event mapper-to-host-to-guest smoke passed on 2026-10-05: host/guest ABI matched, elapsed time 3.173922292 s, peak monitored process-tree RSS 77,709,312 bytes (about 74.1 MiB), with cycles unreported and the 50-million-cycle cap configured. The public watchdog report is `/tmp/eudr-v2-final-guest-20261005.json`; the private witness remains mode 0600 outside Git. This working-tree execute does not establish final committed source identity, cryptographic proof verification or performance evidence. Later guest-affecting changes require a new bounded execute. The watchdog reports incomplete validation on a limit/monitor failure. Do not use local full benchmarks or proving as a substitute.

Contract logic tests use an accepting verifier only inside test compiler input. Run them only against an owned, separate Anvil instance on a confirmed free port and below 256 MiB. The previous checks used port 18545 and a 192 MiB/one-CPU container that was stopped afterward:

```sh
ANVIL_RPC=http://127.0.0.1:18545 node contracts/test/anvil_policy_v2_test.mjs
ANVIL_RPC=http://127.0.0.1:18545 node contracts/test/anvil_policy_server_test.mjs
```

Their `contract-stub-test` output proves contract rejection behavior, not SP1 proof verification or a formal gate. The server test also mines a hash-only baseline receipt, labeled `hash-on-chain`.

## E2 CPU local acceptance

Run in a clean, dedicated checkout whose parent directory is the private `EUDR_RUN_ROOT`. The `--local-smoke` entrypoint checks the **actual generated canonical-event count** is 8–16, requires one seed, CPU, accelerated pacing, a bounded queue and an observed local preflight. A Poisson rate by itself is not an event cap. Use fresh output paths and a unique run ID; never use this output for the formal RQ2 table.

```sh
export EUDR_RUN_ROOT="$(dirname "$PWD")"
export E2_OUTPUT="$EUDR_RUN_ROOT/output"
export E2_RUN_ID=$(date +%s)
export POLICY_MANIFEST="$E2_OUTPUT/e2-local-policy.json"
export ANVIL_IMAGE=ghcr.io/foundry-rs/foundry:stable
docker pull "$ANVIL_IMAGE"
npm ci --prefix contracts
cargo build --locked --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e2_real_benchmark
crates/sp1-e2e/target/release/e2_real_benchmark --local-smoke --accelerated --prover cpu \
  --lambda-events-per-min 16 --duration-min 1 --seeds 1 --run-id "$E2_RUN_ID" \
  --queue-capacity 1 --queue-wait-timeout-sec 1 \
  --prepare-policy-manifest "$POLICY_MANIFEST"
bash scripts/e2_stack.sh up
uv run --python 3.12 python scripts/e2_preflight.py --local-smoke \
  --source-revision "$(git rev-parse HEAD)" --out "$E2_OUTPUT/e2-local-preflight.json"
POLICY_MANIFEST="$POLICY_MANIFEST" crates/sp1-e2e/target/release/e2_real_benchmark \
  --local-smoke --accelerated --prover cpu --lambda-events-per-min 16 --duration-min 1 \
  --seeds 1 --run-id "$E2_RUN_ID" --queue-capacity 1 --queue-wait-timeout-sec 1 \
  --preflight "$E2_OUTPUT/e2-local-preflight.json" --out "$E2_OUTPUT/e2-local.jsonl"
uv run --python 3.12 python scripts/e2_real_analyze.py \
  --raw "$E2_OUTPUT/e2-local.jsonl" --policy-manifest "$POLICY_MANIFEST" \
  --expected-seeds 1 --allow-partial --out "$E2_OUTPUT/e2-local-analysis.json"
```

Check `localComplete=true`, every lot `fabricReadback=true`, the mined SP1 receipt and the final Fabric/Anvil root transition before calling this local pass. The runtime records an exact ledger readback of every event after Fabric acknowledgement and before proving. Preserve output and logs, then run `bash scripts/e2_stack.sh down` only for the stack owned by this checkout. The local preflight marks GPU/CUDA `N/A` with a reason and is rejected by the formal preflight gate; `make e2` remains the separate paced 480/60/30 campaign.

## E3 local CPU gate and paired four-mode smoke

Issue #42 has two local acceptance stages. Stage A is a real two-lot × eight-event recursive SP1 gate and Anvil receipt. Stage B runs the same deterministic nine-event, one-shipment workload through all four modes on **fresh Fabric and Anvil state per mode**. The local path is accelerated, one CPU seed, and cannot populate the formal RQ3 table or Wilcoxon tests. Run serially from a clean, dedicated checkout inside its private `EUDR_RUN_ROOT`; private proof/witness files stay outside Git.

```sh
export EUDR_RUN_ROOT="$(dirname "$PWD")"
export E3_OUTPUT="$EUDR_RUN_ROOT/output"
export ANVIL_IMAGE=ghcr.io/foundry-rs/foundry:stable
mkdir -p "$E3_OUTPUT" "$E3_OUTPUT/modes"
npm ci --prefix contracts
cargo build --locked --release --manifest-path crates/sp1-e2e/Cargo.toml \
  -p sp1-e2e --bin e3_epoch_gate --bin e4_real_cell
export E3_GATE_BINARY="$PWD/crates/sp1-e2e/target/release/e3_epoch_gate"
export E3_MODE_BINARY="$PWD/crates/sp1-e2e/target/release/e4_real_cell"
export POLICY_MANIFEST="$E3_OUTPUT/e3-gate-policy.json"
"$E3_GATE_BINARY" --prepare-only --prover cpu --epoch-id 700001 \
  --events-per-leaf 8 --out "$POLICY_MANIFEST"
bash scripts/e2_stack.sh up
python3 scripts/e3_local_warmup.py --out "$E3_OUTPUT/e3-gate-warmup.json"
"$E3_GATE_BINARY" --prover cpu --epoch-id 700001 --events-per-leaf 8 \
  --out "$E3_OUTPUT/e3-gate-proof.json"
E3_FIXTURE="$E3_OUTPUT/e3-gate-proof.json" ANVIL_RPC=http://127.0.0.1:8545 \
  node contracts/test/anvil_e3_smoke.mjs > "$E3_OUTPUT/e3-gate-receipt.json"
python3 scripts/e3_gate_manifest.py --fixture "$E3_OUTPUT/e3-gate-proof.json" \
  --receipt "$E3_OUTPUT/e3-gate-receipt.json" --git-commit "$(git rev-parse HEAD)" \
  --out "$E3_OUTPUT/e3-gate-manifest.json"
bash scripts/e2_stack.sh down
```

The stack down command leaves ignored generated Fabric crypto/runtime in place. After collecting and checking the gate evidence, call `bash infra/fabric/raft/bootstrap.sh down`, inspect the exact `infra/fabric/raft/runtime` directory owned by this checkout, and remove it before starting a new stack. Do this after each paired mode as well. A cold chaincode may exceed the first 30-second endorsement deadline; the excluded warmup retries only after checking whether its sentinel event was committed.

Prepare the *separate* paired workload manifest from the same clean source revision. The one-seed Poisson schedule yields nine events with `np=4`, shipment size 16 and one 60-second epoch; local execution skips waiting for due times but preserves their deterministic order and digest. Rotate modes for seed 1 as `hash-on-chain`, `vecro-adapted`, `fabric-only`, `proposed`:

```sh
"$E3_MODE_BINARY" --local-smoke --mode proposed --prover cpu --seed 1 \
  --np 4 --shipment 16 --epoch-s 60 --epochs 1 \
  --prepare-policy-manifest "$E3_OUTPUT/e3-local-policy.json"
export POLICY_MANIFEST="$E3_OUTPUT/e3-local-policy.json"
reset_e3_runtime() {
  bash infra/fabric/raft/bootstrap.sh down
  python3 - <<'PY'
from pathlib import Path
import shutil
repo = Path.cwd().resolve()
runtime = (repo / "infra/fabric/raft/runtime").resolve()
assert runtime == repo / "infra/fabric/raft/runtime" and (repo / ".git").exists()
if runtime.is_dir():
    shutil.rmtree(runtime)
PY
  python3 - <<'PY'
import socket, time
ports = (7050, 7051, 7053, 8050, 8053, 9050, 9053, 9051, 8080, 8545, 8546)
for _ in range(90):
    free = True
    for port in ports:
        with socket.socket() as connection:
            try:
                connection.bind(("127.0.0.1", port))
            except OSError:
                free = False
    if free:
        break
    time.sleep(1)
else:
    raise SystemExit("E3 stack ports are still in use")
PY
}
reset_e3_runtime
for mode in hash-on-chain vecro-adapted fabric-only proposed; do
  bash scripts/e2_stack.sh up
  python3 scripts/e3_local_warmup.py --out "$E3_OUTPUT/$mode-warmup.json"
  uv run --python 3.12 python scripts/e3_local_run.py \
    --binary "$E3_MODE_BINARY" --gate "$E3_OUTPUT/e3-gate-manifest.json" \
    --policy-manifest "$POLICY_MANIFEST" --mode "$mode" \
    --allocation-root "$EUDR_RUN_ROOT" --out "$E3_OUTPUT/modes/$mode.json"
  bash scripts/e2_stack.sh down
  reset_e3_runtime
done
```

After all four modes finish, give the analyzer a directory containing only the four mode records. Keep warmup, budget and audit JSON files in `modes/` so they cannot be mistaken for mode records:

```sh
mkdir -p "$E3_OUTPUT/paired-records"
for mode in hash-on-chain vecro-adapted fabric-only proposed; do
  ln -s "../modes/$mode.json" "$E3_OUTPUT/paired-records/$mode.json"
done
uv run --python 3.12 python scripts/e3_local_analyze.py \
  --raw-dir "$E3_OUTPUT/paired-records" --out "$E3_OUTPUT/e3-local-analysis.json"
```

The mode runner records actual workload digest, source revision, gate hash, receipt and observed container IDs. The local analyzer requires all four distinct stacks and the same paired workload, and emits `localComplete`; the formal `e3_analyze.py` rejects these records even with `--allow-partial`. Verify the gate receipt/root and each mode's L1 scope before publishing public summaries. Keep private raw proof, manifests, full logs and signing material outside Git.

## Trusted configuration before measurement

Trusted manifest preparation invokes SDK setup and workload/issuer preparation. It performs no proving or network posts, but SDK setup can be expensive: prepare it on the actual HPC host, from final reviewed binaries, before timed runs. Select exact epoch IDs, event counts, seeds, axes and unique run IDs up front.

Allocation-only examples, using already-built binaries and new external output paths:

```sh
E3_BINARY=/reviewed/bin/e3_epoch_gate
E2_BINARY=/reviewed/bin/e2_real_benchmark
E4_BINARY=/reviewed/bin/e4_real_cell
"$E3_BINARY" --prepare-only --prover cpu --epoch-id 700001 --events-per-leaf 8 \
  --out /outside/git/gate-policy.json
"$E3_BINARY" --prepare-only --prover cpu --epoch-id 700002 --events-per-leaf 64 \
  --out /outside/git/preflight64-policy.json
"$E4_BINARY" --prover cpu --np 125 --shipment 64 --epoch-s 60 --epochs 60 --seed 0 \
  --prepare-policy-manifest /outside/git/e3-seed0-policy.json
python3 scripts/policy_manifest.py --out /outside/git/trusted-combined-policy.json \
  /outside/git/gate-policy.json /outside/git/preflight64-policy.json /outside/git/e3-seed0-policy.json
"$E2_BINARY" --prover cpu --lambda-events-per-min 480 --duration-min 60 --seeds 30 \
  --run-id 900001 --prepare-policy-manifest /outside/git/e2-policy.json
```

Prepare every intended E3 seed and E4 cell before merging their snapshots. The short example covers only the displayed workloads. Preflight uses the same two epoch IDs for paired CPU/GPU repetitions and must use the matching n=8/64 snapshots. Generic merge preserves approved pairs, common keys and E3/E4 workload descriptors. **Keep the original E2 manifest**: its `runId`, `workloadSeeds`, schedule hashes and configured load are required by the E2 runner/analyzer and are not preserved by generic merge.

The selected manifest must have `protocolVersion: 2`, approved root pairs (`acceptedPolicies` or one top-level pair), `leafProgramVKey`, `aggregateProgramVKey`, `leafVKeyDigest` and `aggregateVKeyDigest`. Roots must be configured independently of submitted proof requests. Server startup registers those pairs before measurements; reset requests cannot register roots or select untrusted keys. Baseline-only startup without program keys cannot deploy a proof anchor.

```sh
POLICY_MANIFEST=/outside/git/trusted-combined-policy.json \
  node contracts/test/anvil_sp1_anchor_server.mjs
```

This starts a service and deploys configuration; it is not a lightweight read-only check. Use it inside the allocation-owned stack. Registration/deployment gas and setup time stay outside the timed proof/anchor window.

## HPC dry-run job preparation

Do not infer a GPU model, allocation, partition, account, CUDA version or Docker permission from repository code. Supply an actual allocation configuration. Current Fabric scripts require Docker; another container runtime has no adapter. Fixed names/ports require concurrency 1 on a compute host.

Example configuration schema; replace every placeholder with observed allocation information before use:

```json
{
  "partition": "REPLACE_WITH_ALLOCATED_PARTITION",
  "account": "REPLACE_WITH_ALLOCATED_ACCOUNT",
  "gres": "REPLACE_WITH_ALLOCATED_GRES",
  "time": "01:00:00",
  "container_runtime": "docker",
  "compute_host": "REPLACE_WITH_ALLOCATED_COMPUTE_HOST",
  "memory_gib": 64,
  "cpus": 4,
  "prover": "cpu",
  "concurrency": 1,
  "policy_manifest": "/outside/git/trusted-combined-policy.json",
  "commands": {
    "e1": [["python3", "scripts/e1_plan.py", "--output-root", "/outside/git/e1-planned"]]
  }
}
```

The displayed resources are schema examples, not granted capacity. This `e1` command only plans a campaign. Supply explicit argv arrays for the other selected phase (`gate`, `preflight`, `e2`, `e3`, `e4`) after reviewing its prerequisites. E3/E4 job generation requires a passed real-v2 gate from the exact source revision.

```sh
python3 scripts/hpc_jobs.py --config /outside/git/actual-hpc-config.json \
  --phase e1 --runtime-root /outside/git/allocation-runs \
  --out /outside/git/new-e1-job.sh
bash -n /outside/git/new-e1-job.sh
python3 scripts/e1_plan.py --output-root /outside/git/e1-planned
python3 scripts/e3_plan.py --out /outside/git/new-e3-plan.csv
```

`hpc_jobs.py` writes scripts and reports `submitted=false`; it never submits them. Generated jobs clone and detach a committed revision into a fresh workspace, retaining Git provenance. They pin the compute hostname, require an allocation, lock shared topology access and retain outputs. Built-in phase commands and the sequential clone/configure/submit/collect workflow are documented in [Trí's HPC handoff](HPC_H200_TRI_RUNBOOK.md). Run gate → prepare catalog → preflight → campaigns serially when they share the topology. `scripts/e2_stack.sh up|down` requires an allocation-owned workspace; collect raw records, manifests, receipts and private archives before cleanup. Failed/uncollected E3 cells retain their topology for recovery. A generated script is not an accepted job or a passed preflight.

For actual CUDA runs, the existing binaries must have been built with `--features cuda` and commands must explicitly select `--prover cuda`. Merely requesting a GPU or calling a CPU-default Make target does not run CUDA. Record the observed GPU, including H200 if that is what was allocated, rather than the paper's example A100.

## Gate, campaigns and paper output

After approved HPC execution produces a real aggregate fixture, the gate stages are:

```sh
POLICY_MANIFEST=/outside/git/trusted-combined-policy.json \
  E3_FIXTURE=/outside/git/real-gate-fixture.json ANVIL_RPC=http://127.0.0.1:8545 \
  node contracts/test/anvil_e3_smoke.mjs > /outside/git/new-gate-receipt.json
python3 scripts/e3_gate_manifest.py --fixture /outside/git/real-gate-fixture.json \
  --receipt /outside/git/new-gate-receipt.json --git-commit "$SOURCE_REVISION" \
  --out /outside/git/new-gate-manifest.json
```

Use new paths, and check redirection outputs before retrying; shell redirection can overwrite a file even when the program refuses to overwrite its own inputs. The offline gate binds v2/source/program/root/count/log metadata and requires recorded aggregate tamper rejection; it relies on the real smoke producer and does not cryptographically reverify a JSON receipt on its own.

Formal targets remain: E1 C1–C5 8/16/32/64 events ×30 seeds at depths 6/16; wrapper at least 8/64 ×10 seeds; SP1 CPU/CUDA 8/64 plus two-lot aggregate with at least five repetitions; E2 480 events/min ×60 minutes ×30 seeds; E3 four modes ×30 paired seeds; E4 80 screening plus 270 confirmation runs, four epochs per run. Do not use legacy `e2-bench` or accelerated/debug pilots to populate RQ2.

`e2_preflight.py` collects observed same-host Fabric/Anvil, hardware and toolchain metadata; pass its new external record with the E2 runner's `--preflight` argument. `e4_preflight.py` checks actual topology, paired workloads and timings. The gate fixture's `aggregateGroth16ProofMs` currently combines recursive proving and SDK Groth16 wrapping; unavailable separate phase/RSS metrics remain null. Do not label this combined value as an independently measured wrapping-only time. E3 `--allow-partial` reports cannot produce formal p-values; E4 mock executor data is schema/test evidence only. Preserve saturated/censored runs and missing-data reasons. Gas/USD requires actual receipt gas plus dated, sourced gas-price/ETH assumptions through `scripts/gas_cost.py`; it reports an estimate, not paid mainnet fees.

## Selective disclosure and private archives

`EUDR_PRIVATE_ARCHIVE` is an optional fresh absolute directory outside the allocation workspace. In the implemented E4 archive path, the real runner writes `lot-ID.bin` private v2 witnesses and `lot-ID-authentication.json` ordered public paths after attaching the actual receipt transaction hash. Files are new-only and mode 0600 on Unix. No real archive has been produced by the local fixture tests.

```sh
/reviewed/bin/view-open --lot 17 --preset certifier \
  --witness /private/archive/lot-17.bin \
  --aggregation-path /private/archive/lot-17-authentication.json \
  --out /private/new-view.json
/reviewed/bin/view-verify --bundle /private/new-view.json \
  --trust /trusted/expected-anchor.json --rpc https://independently-selected-rpc
```

The [disclosure schema](../crates/disclosure/README.md) defines presets, exact bottom-up child paths and external chain/anchor/registry/verifier/program/root trust. At least one event is required to authenticate a claimed lot. Only selected events, their own salts/paths and requested polygon/credential openings are exported; nullifier keys, batch nonces, signatures and unselected event bytes remain private. Independent trust includes the real verifier runtime code hash; do not derive trust from the bundle sender. Prototype reports include bundle bytes and verify time with `formal=false` and official thirty-repetition measurements pending.

Sepolia deployment defaults to dry-run, with no RPC/key access. The five-epoch script requires five real fixtures. No deployment, testnet receipt/inclusion delay, `finaltrue`, submission tag or paper submission has occurred in this readiness work. C1 plot/multi-polygon/tile completeness and physical lot continuity remain future work; author ORCID/funding information remains pending.

Paper PDF validation is incomplete: the initial Tectonic format download was stopped after approximately five minutes; no TeX Live installation was found. The updated TeX source and red placeholders are preserved.
