# E4 policy v2: four-epoch local CPU measurement, 2026-10-09

## Scope and provenance

Issue #43 was run on an Apple M1 Pro with 8 logical CPUs and 32 GiB RAM, using source commit `d4200141b8aed5f23f67ea8c5f602b925f72afc2`. The run used real Fabric commits, SP1 CPU compressed leaf and recursive aggregate Groth16 proofs, and Anvil transactions. The separate E3 gate was repeated at the **same commit and host** before E4. [Provenance](provenance.json) records binary, image and private artifact hashes. Proofs, raw shards, logs and Fabric material remain at the private output root; they are not committed.

This is `e4-local-smoke`, with `formalComplete=false`. It uses one bounded configuration and one deterministic seed. The `epoch_s=60` schedule is **accelerated**: the runner preserves event ordering and the four logical epoch boundaries, while skipping due-time waits. Therefore the wall-time throughput below measures this local CPU run, not a paced arrival rate or a scalability curve. There is no CUDA GPU on this host. The formal screening and confirmation campaign, including the paper's 80+270 runs, remains pending.

## Gate and measured run

The [E3 gate](gate.json) acknowledged and independently read back 16/16 Fabric events in two lots. It produced two compressed leaf proofs and one recursive Groth16 proof, rejected changed proof/public values, and received an Anvil status-1 transaction. Its watchdog completed in 2,479.58 s with 15.85 GiB peak process-tree RSS. E4 then started on a fresh stack with the trusted [policy manifest](policy.json), `np=3`, shipment size 16, four 60-second logical epochs, seed 3, proposed mode, CPU prover and two Rayon threads. The generated event counts were 13, 14, 12 and 9; each epoch contained one nonempty lot and one shipment.

| Measurement | Local result |
| --- | ---: |
| Events committed / offered | 48 / 48 |
| Shipments committed / offered; completion | 4 / 4; 100% |
| Epochs completed; queue drained | 4 / 4; yes |
| Measured wall time | 7,430.90 s (2 h 3 min 50.90 s) |
| Throughput | 0.0064595 events/s; 0.0322976 shipments/min |
| Compressed leaf proof time, mean / median | 696,484.5 / 722,530.5 ms (4 samples) |
| Recursive aggregate proof time, mean / median | 1,154,356.25 / 1,154,872.5 ms (4 samples) |
| Audit latency, mean / median | 4,721,074 / 4,747,517.5 ms (4 lots) |
| Gas total / per shipment | 1,205,168 / 301,292 |
| Calldata total / per shipment | 3,728 / 932 bytes |
| E4 watchdog elapsed / peak process-tree RSS | 7,482.41 s / 19.94 GiB |

The [analysis](analysis.json) includes minima and maxima. Audit latency is measured from Fabric acknowledgment of each lot to its on-chain receipt. The lots enter Fabric early, then CPU proofs execute sequentially; later epochs therefore have longer audit latency. It is one sample **per lot**, not per event. Gas and calldata are divided by four shipments; this configuration happens to have one shipment per epoch.

| Epoch | Events | Leaf proof | Aggregate proof | Audit latency | Gas | Calldata |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3000 | 13 | 713,490 ms | 1,262,830 ms | 1,976,700 ms | 301,304 | 932 B |
| 3001 | 14 | 768,691 ms | 1,113,534 ms | 3,857,162 ms | 301,268 | 932 B |
| 3002 | 12 | 731,571 ms | 1,044,850 ms | 5,637,873 ms | 301,304 | 932 B |
| 3003 | 9 | 572,186 ms | 1,196,211 ms | 7,412,561 ms | 301,292 | 932 B |

## Independent integrity checks

- [Fabric readback](ledger-check.json) fetched all 48 canonical 86-byte records by event ID, checked their hashes, and reconstructed the trusted workload digest. The chaincode warmup sentinel was excluded from the measurement.
- [Anvil RPC checks](receipts.json) found four status-1 transactions in blocks 5–8, matched measured gas and calldata, confirmed contiguous old/new nullifier roots and verified the contract's final root. [Decoded logs](event-logs.json) independently matched `EpochAggregated` epoch IDs, commitments, roots, lot counts and event counts.
- The strict local merge rejected missing or duplicate epoch shards, a bad receipt, wrong host and a partial run; see [negative checks](negative-merge-check.json). The [formal analyzer rejection](formal-rejection.json) confirms this local validation kind cannot enter the formal E4 sweep.
- Both [resource budgets](budgets) passed. A synthetic gas-to-USD fixture and synthetic Figure 2 PGFPlots generation exercised the export path; [pipeline smoke metadata](pipeline-smoke.json) marks these as synthetic. They are not paper cost or figure results.

## Reproduction and interpretation

Use the [local CPU runbook](../../../docs/reports/e4_local_cpu_runbook.md) for the clean-stack commands and trusted-policy checks. The final E4 invocation used `e4_real_cell --np 3 --shipment 16 --epoch-s 60 --epochs 4 --seed 3 --prover cpu --mode proposed --e4-local-smoke --wall-time-s 21600`, with `POLICY_MANIFEST` pointing to the private E4 policy and `RAYON_NUM_THREADS=2`. `scripts/e4_local.py` wrote four epoch shards, then merged and analyzed them. The final stack was torn down after independent RPC checks.

This run establishes local pipeline integrity and bounded CPU measurements for one point. It does not estimate GPU speedup, saturation, policy trade-offs across cells, confidence intervals, or formal paper figures. The accelerated wall-time throughput and synthetic export checks must not be inserted into the formal E4 paper table.
