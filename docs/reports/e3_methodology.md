# E3 methodology and data contract

This document defines the implementation and evidence boundary for [issue #21](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/21). It does not report measurements until the recursive proof gate and the main workload have completed.

## Recursive epoch statement

The E3 leaf guest reuses the E2 C1–C5 policy checks, but derives a separate sparse nullifier identity from `(epochId, shipmentId, leafIndex)`. Every canonical event keeps the same `lotId = shipmentId`. This lets a 128-event shipment use two 64-event leaves, with a valid old-root to new-root transition for each leaf. The E2 program and `EpochAnchor.sol` are unchanged.

The aggregate guest receives 1–8 compressed leaf proofs through SP1's proof-input channel and verifies each against a leaf vkey baked into the aggregate ELF. It then requires one epoch ID, a continuous nullifier-root chain, strictly increasing shipment IDs, consecutive leaf indices within each shipment, and complete shipment leaf groups. It folds domain-separated leaf commitments in that exact order. The parent public values are ABI encoded as `(uint64 epochId, bytes32 oldNullifierRoot, bytes32 newNullifierRoot, bytes32 epochCommitment, uint32 lotCount, uint32 eventCount, bool valid)`. `EpochAggregateAnchor.sol` verifies the resulting Groth16 proof, checks the current root, and permits only one anchor per epoch.

Leaf bytes and input formats are versioned in `crates/sp1-e2e/epoch-policy/src/lib.rs`. The leaf vkey is derived from the compiled leaf ELF in the build script, embedded in the aggregate guest, and checked again by the host. This binding is necessary: an aggregate that verifies a caller-selected leaf vkey would not prove the intended policy.

## Four compared systems

| Name | Implemented audit-ready condition | EVM evidence |
| --- | --- | --- |
| Fabric-only | All canonical events committed and readable through Fabric Gateway | None; gas and calldata are N/A |
| Hash-on-chain | One domain-separated digest of all canonical shipment events stored and read back | One Anvil transaction and receipt per shipment |
| VeCroToken-derived adapted | Shipment digest is minted as an ownership token, transferred to a fixed auditor address, and both state fields are read back | Mint and transfer Anvil transactions and receipts per shipment |
| Proposed | Fabric acknowledges the canonical events; SP1 leaf proofs are recursively checked in the epoch guest; the epoch commitment and root are stored and read back | One Groth16 aggregate anchor receipt per epoch |

The VeCroToken-derived adapter is **an adaptation, not a reproduction** of the original cross-chain, attribute-encryption protocol. It only tests the cost of the tokenized ownership/digest path implemented here. No USD, mainnet, or original VeCroToken performance claim can be inferred from its Anvil numbers.

## Paired workload and manifest

For each seed, the runner deterministically creates a Poisson arrival schedule and truncated-normal lots from the existing E2 workload generator. Every event is an 86-byte canonical `EpcisEventV1` plus a real Ed25519 signature. Producer IDs are assigned cyclically over 125 producers; each producer has a deterministic seed-specific signing key. The manifest digest includes each shipment ID, epoch ID, due time, producer ID, canonical bytes and signature. All four runs for a seed must report the **same** `manifestSha256` and `expectedShipments` or the analyzer rejects the comparison.

The main run uses 125 producers, λ = 480 events/minute, 60 minutes of real Poisson pacing and 30 paired seeds per system. `--accelerated` is only for pilot debugging. Each baseline needs its own clean state; in particular Fabric-only and Proposed must use separate Fabric ledgers or reset the Fabric ledger between replays because canonical event IDs must remain identical. The runner records Git commit/dirty state, hardware label, SP1 circuit version, contract addresses/chain ID, vkeys, workload parameters and command line.

The Proposed raw receipt also records elapsed time for each uncached compressed leaf proof, the aggregate Groth16 proof, the full prove-and-local-verify call, and parent proof bytes. These stage timings are diagnostic and do not replace the shared audit-latency metric.

For every shipment, the runner writes one raw JSONL row, including failures and timeouts. The denominator for completion rate is every shipment in the shared manifest. An `ok` shipment must have a Fabric readback or valid Anvil receipt and state readback. `timeout` means completion happened after the common arrival period plus the configured drain window. Throughput is completed shipments and their canonical events before the end of the 60-minute arrival period, each divided by 60 minutes. Audit latency is wall-clock time from the final event's scheduled release in the shared manifest to the baseline's audit-ready condition. Fabric acknowledgement is recorded separately, so backlog and ingest delays remain visible. Gas and calldata are taken from Anvil transactions; for Proposed, epoch gas and calldata are allocated evenly over the distinct shipments proved in that epoch, while the raw row also keeps epoch totals.

`scripts/e3_analyze.py` requires all four raw files and verifies one complete row per shipment for each seed. It computes per-seed completion, throughput, median audit latency, mean gas/shipment and mean calldata bytes/shipment, then reports median, IQR, p95 and a 10,000-resample bootstrap 95% CI of the per-seed median. It also reports pooled shipment-level median/IQR/p95 separately. Wilcoxon signed-rank tests use paired seed values; Bonferroni uses the number of applicable baseline pairs per metric. N/A gas/calldata for Fabric-only are excluded rather than recorded as zero. Ties, missing successful observations and the number of paired seeds are explicit in output.

## Reproduction commands

Start Fabric Gateway and Anvil independently, then run `npm run anvil:e3-server` from `contracts/`. Pass `--fabric-gateway <url>` to Fabric-only and Proposed. Use independent clean Fabric deployments for those two replays. For a pilot, run `e3-compare --baseline <name> --accelerated --out results/e3/raw/<name>.jsonl` once per name. For evidence, run the same command with `--main --duration-min 60 --seeds 30 --hardware-label '<machine description>'` and no `--accelerated`. Use the **same** `--run-id`, arrival parameters and drain window in all four commands.

The Proposed runner accepts `--prover cpu` or `--prover cuda`; CUDA requires building with `--features cuda` on an allocated supported GPU. Record the prover and hardware label for every run. Proving time is measured by the real unmodified proving path in both modes.

Analyze with:

```bash
python3 scripts/e3_analyze.py \
  --fabric-only results/e3/raw/fabric-only.jsonl \
  --hash-on-chain results/e3/raw/hash-on-chain.jsonl \
  --vecro-token-adapted results/e3/raw/vecro-token-adapted.jsonl \
  --proposed results/e3/raw/proposed.jsonl \
  --out results/e3/summary.json
```

Raw JSONL and generated proof fixtures stay outside Git. Commit the schema, scripts, methodology and validated main summary/metadata only after the full gate and all 30 paired seeds pass. Pilot output must never be labelled as paper evidence.
