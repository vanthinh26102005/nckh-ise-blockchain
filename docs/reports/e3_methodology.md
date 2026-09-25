# E3 — recursive epoch gate and paired baseline comparison

This is an implementation protocol, **not** a measured result. The paper's E3
tables stay empty until all real runs and receipts pass validation.

## Proof and settlement gate

The existing E2 `EpochAnchor` remains unchanged. E3 proves two independently
committed Fabric lots (eight EPCIS events each) as compressed SP1 policy proofs.
The epoch guest verifies the child proofs, checks a shared epoch, ordered
nullifier-root transitions and commitments, then publishes an aggregate Groth16
proof. It permits at most eight children per node; larger epochs reduce through
recursive levels. A 128-event shipment is split into two 64-event leaves.

The aggregate ABI publishes the requested seven values—epoch ID, old/new roots,
epoch commitment, lot count, event count and validity—plus **two VK digests**.
Those digests bind the leaf and aggregate programs; without them the guest could
be handed unrelated child verification keys. `EpochAggregateAnchor` verifies the
Groth16 proof, key digests and old root, then anchors each epoch only once.
The gate is passed only after the Fabric writes, local proof verification and an
Anvil transaction with status 1 and the expected new root. Raw proof bytes and
runtime state belong outside Git.

## Four paired modes

Every mode uses canonical `EpcisEventV1`, the same ChaCha20-seeded Poisson arrival
schedule, 125 producers × 3.84 events/min (= 480/min), 60 minutes, shipment
size 64 and 30 paired seeds. Epoch length is 60 seconds. Modes use distinct
IDs so earlier Fabric writes cannot satisfy later acknowledgements. Rotate mode
order by seed (four-way counterbalance) to reduce host/ledger-growth order bias,
and record the actual run order in deployment metadata.

| Mode | Auditable state | L1 treatment |
|---|---|---|
| Fabric-only | Fabric commit/query | No L1 transaction; gas/bytes are N/A, not zero |
| Hash-on-chain | Fabric + per-shipment SHA-256 digest | One real Anvil receipt per shipment |
| VeCroToken-adapted | Fabric + SP1 C1–C5 leaf proof + token ID | One real Anvil proof verification/mint per leaf |
| Proposed | Fabric + SP1 compressed leaf proofs + recursive epoch proof | One real Anvil receipt per epoch |

**VeCroToken-adapted is an explicit comparison adapter**, approved for this
artifact; it is not a reproduction of VeCroToken's original cross-chain,
attribute-encryption or token protocol. It uses our SP1 policy statement and a
minimal token anchor, so any paper table must use the label *adapted* and state
this difference. Its per-leaf gas may not be comparable to the original system.

The audit clock starts at the Fabric acknowledgement and ends at the receipt
when L1 exists. Fabric-only reports local availability and must not be described
as L1 settlement latency. Calldata/gas are amortized by completed shipment,
with Fabric-only as N/A. Record CPU/GPU prover choice, Git commit, gate receipt,
elapsed time, offered/committed counts, all transaction receipts and censoring.

## Statistical gate

`scripts/e3_run.py` runs **one mode and one seed** per invocation, saving an
immutable JSON file outside the repository. `scripts/e3_analyze.py` refuses a
formal comparison unless all four modes have exactly seeds 0–29, all 120 runs
completed 60 epochs, and none is censored/saturated. It reports median, IQR,
p95 and 10,000-resample 95% bootstrap CI of the mean. Six paired throughput
comparisons use exact two-sided Wilcoxon signed-rank p-values with Bonferroni
correction. Cliff's delta is reported as a descriptive effect size. Pilot data
can be inspected with `--allow-partial`; that path emits **no** formal p-values.
Run one mode/seed at a time per shared Fabric/Anvil topology: each run resets
its anchor and concurrent runs would race on that state. Parallel Slurm jobs
need isolated topology, ledger and ports, not merely distinct output files.

Example after a passed gate, built CUDA binary and running Fabric/Anvil services:

```sh
python3 scripts/e3_run.py --binary crates/sp1-e2e/target/release/e4_real_cell \
  --gate results/e3/gate_manifest.json --mode proposed --seed 0 \
  --out /datastore/uitchain/e3/raw/proposed-0.json \
  --git-commit "$(git rev-parse HEAD)"
python3 scripts/e3_analyze.py --raw-dir /datastore/uitchain/e3/raw \
  --out results/e3/comparison.json
```

These commands illustrate paths only; no A100 formal run is claimed here.
