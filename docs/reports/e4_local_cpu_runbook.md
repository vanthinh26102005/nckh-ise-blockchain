# E4 local CPU acceptance, issue #43

This run is a bounded single-cell check of the real proposed pipeline on one
Apple M1 Pro. It is **not** the 80 screening + 270 confirmation formal RQ4
campaign. Formal E4 still requires a measured same-host CPU/GPU preflight,
paced Poisson arrivals and the full predefined grid.

## Fixed local workload

- SP1 6.4.0 CPU, Fabric 2 org / 2 peer / 3 Raft orderer, Anvil.
- Seed 3, 3 producers, shipment size 16, four logical 60-second epochs.
- `e4_real_cell --e4-local-smoke` accelerates due-time waits while preserving
  the ChaCha20 Poisson schedule, event order and workload digest. Each epoch
  must offer 8–16 events and make exactly one leaf and one shipment. The
  trusted policy manifest records the actual per-epoch counts before the run.
- The binary verifies the SP1 proof, obtains one mined Anvil receipt per epoch,
  and records per-epoch leaf/aggregate time, leaf acknowledgement-to-receipt audit latency, gas and
  calldata. A finite `--wall-time-s` and the process watchdog bound the run.
- `scripts/e4_local.py shard` checks source, passed E3 gate, trusted registry,
  keys, workload digest, mined receipts and exact count reconciliation. A
  complete drained run writes four raw JSONL
  shards outside Git. `merge` rejects missing/duplicate/mismatched shards,
  partial runs, noncontiguous roots and tampered receipts. `analyze` reports
  local throughput, completion, proving and audit times, gas and calldata per
  **shipment**. GPU/CUDA is N/A with an explicit reason.
  A censored/saturated run keeps each already mined epoch shard plus a
  `partial-status.json` with the stop reason and missing metrics as N/A; merge
  refuses to call it complete.

## Reproduction order

Use a clean checkout at the **code-fix commit**. Keep `$E4_PRIVATE` outside the
Git checkout and start with fresh output paths. The E3 gate and E4 cell must
embed the same Git revision. The E3 gate uses a two-lot fixture with its own
registry roots; its verification keys and source revision must match E4. The
E4 workload has a separate trusted registry pair and is validated against its
own prebuilt manifest.

1. Build `e3_epoch_gate` and `e4_real_cell` in release mode from the clean
   commit. Generate the gate policy with `e3_epoch_gate --prepare-only
   --epoch-id 700001 --events-per-leaf 8 --prover cpu --out
   "$E4_PRIVATE/e3-gate-policy.json"`.
2. Start the E2 Fabric/Anvil stack with `POLICY_MANIFEST` set to that gate
   policy. Warm chaincode using `scripts/e3_local_warmup.py`. Run the real
   `e3_epoch_gate`, submit its fixture to `contracts/test/anvil_e3_smoke.mjs`,
   and create the passed manifest with `scripts/e3_gate_manifest.py`.
   Check Fabric readback and Anvil RPC, then tear the stack down.
3. Build the local E4 policy with
   `e4_real_cell --np 3 --shipment 16 --epoch-s 60 --epochs 4 --seed 3
   --prover cpu --e4-local-smoke --prepare-policy-manifest
   "$E4_PRIVATE/e4-policy.json"`. Start a fresh stack with this policy and
   warm chaincode outside the measurement.
4. Run `e4_real_cell` with the same axes, `--mode proposed`,
   `--wall-time-s 21600`, `POLICY_MANIFEST` set to the E4 policy, and a
   process watchdog. Save stdout outside Git. Preserve failures as failures;
   do not replace a censored or saturated epoch with a successful fixture.
5. Run `scripts/e4_local.py shard --run "$E4_PRIVATE/e4-run.json"
   --policy "$E4_PRIVATE/e4-policy.json" --gate
   "$E4_PRIVATE/e3-gate-manifest.json" --shards "$E4_PRIVATE/shards"`.
   Then `merge` with the same policy/gate/shards and `--merged
   "$E4_PRIVATE/merged.json"`; finally `analyze --merged
   "$E4_PRIVATE/merged.json" --policy "$E4_PRIVATE/e4-policy.json" --gate
   "$E4_PRIVATE/e3-gate-manifest.json" --report "$E4_PRIVATE/report.json"`.
   Analysis rechecks the trusted source/workload, receipt chain and measured
   gas/calldata against the receipts. Verify independent Fabric readback for every
   offered event and Anvil RPC receipt/root for all four epochs. Publish only
   selected metrics and hashes; raw proof, witnesses, full logs and Fabric
   crypto stay private.

The formal `e4_run.py` and `e4_analyze.py --require-real` continue to require
the complete preflight/grid. Local evidence is tagged `e4-local-smoke` and
cannot satisfy the formal validator. No local number should fill Figure 2 or
the paper's formal RQ4 table.
