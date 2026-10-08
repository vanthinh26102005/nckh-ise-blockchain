# E3 policy v2: local CPU gate and paired four-mode smoke, 2026-10-08–09

## Scope

Issue #42 local acceptance ran from clean source commit `94affcd733f2d0727e8943de1e449d7e3231029d` on an Apple M1 Pro (8 logical CPUs, 32 GiB RAM, macOS 26.5 arm64). It used SP1 6.4.0 CPU proving, Fabric 2.5.16 with 2 organizations, 2 peers and 3 EtcdRaft orderers, and Anvil 1.5.1 on chain 31337. [Provenance](provenance.json) records the binary, image and private artifact hashes. Raw proofs, full logs and trusted manifests remain outside Git at the private output root listed there.

This is **one accelerated local seed**, not the paced GPU/HPC RQ3 campaign. The [local analysis](analysis.json) reports `localComplete=true`, `formalComplete=false` and no paired statistical tests. The formal 4 modes × 30 seeds, 480 events/minute, 60-minute run remains pending. VeCro here is this repository's **adapted** implementation.

## Real recursive gate

The gate committed and read back **16 canonical Fabric events in two lots of eight**. Two compressed leaf proofs verified, a modified child was rejected, and SP1 produced a recursive aggregate Groth16 proof. The real `SP1VerifierGroth16` contract accepted the 352-byte public values with a status-1 `EpochAggregated` receipt. The contract test rejected a changed aggregate proof, changed public values, wrong old root, wrong leaf key and epoch replay before reporting success. [Gate metadata](gate.json), [public receipts](receipts.json), [independent Fabric readback](ledger-check.json) and [independent Anvil RPC/root check](chain-check.json) record the evidence.

| Gate measurement | Local result |
| --- | ---: |
| Committed/read-back events; lots | 16/16; 2 |
| Compressed leaf proof times | 548,267 ms; 546,354 ms |
| Aggregate Groth16 proof time | 1,342,779 ms |
| Gate transaction | `0xf03731d970eae2bb573a42659a5109fb22c99062da3b0689f2a8885a4b0152b9` |
| Gate gas / calldata | 301,316 gas / 932 bytes **per two-lot epoch** |
| Whole gate wall time / peak process-tree RSS | 2,522.53 s / 14.82 GiB |

The gate transaction has a different two-lot workload from the paired one-shipment smoke, so its gas is reported per gate epoch and is not a fifth comparison row.

## Paired local smoke

All four modes used seed 1, `np=4`, shipment size 16, one 60-second logical epoch, CPU prover and the same deterministic **9-event, 1-shipment** schedule. Local execution skipped Poisson due-time waits but kept event order and workload digest `0xd227f25df5957f94f4d779c420eedfd843cd0d6a111dfb3644cb6cd20bcd15e1`. The run order was `hash-on-chain`, `vecro-adapted`, `fabric-only`, `proposed`. Each mode used a freshly rebuilt Fabric ledger and a distinct Anvil container; an excluded sentinel event warmed chaincode before measurement. Every mode committed 9/9 events and drained its epoch. Independent GETs checked all nine 86-byte event records and SHA-256 digests per mode.

| Mode | Wall time | Shipments/min | Events/s | Audit latency | Gas/shipment | Calldata/shipment |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Fabric only | 18.31 s | 3.2772 | 0.4916 | 0 ms | N/A | N/A |
| Hash on chain | 18.38 s | 3.2646 | 0.4897 | 85 ms | 46,907 | 100 B |
| VeCro adapted | 1,729.61 s | 0.03469 | 0.005203 | 1,711,253 ms | 298,870 | 868 B |
| Proposed | 1,884.39 s | 0.03184 | 0.004776 | 1,866,069 ms | 301,280 | 932 B |

Audit latency is one observation per mode. `fabric-only` recorded 0 ms because that path converts a near-immediate interval to integer milliseconds; it does not imply zero physical latency. Throughput here reflects these short accelerated runs and CPU proving, not the formal load target or a confidence interval. The three on-chain modes have status-1 mined receipts whose gas and calldata match independent Anvil RPC queries; the two proof modes also have an on-chain nullifier root matching their receipts. `fabric-only` has no measurement receipt or gas/calldata sample by design.

## Reproduction and limits

The [runbook](../../../docs/POLICY_V2_RUNBOOK.md) contains the local commands and the clean-stack sequence. For analysis, use a directory containing **only the four mode record JSON files**, such as the private `paired-records` directory, then run `scripts/e3_local_analyze.py --raw-dir …`. The formal `e3_analyze.py --allow-partial` was explicitly tested against these records and rejected them. One stack restart had to wait for macOS port 8080 to be released; it had no measured events. A first extra Fabric readback audit timed out after proving, and its retry verified all nine records. No failed or censored mode was omitted.

The private proofs, manifests, complete logs, per-mode raw records and detailed resource reports are identified by SHA-256 in [provenance.json](provenance.json). Public [budgets](budgets) show all five successful watchdog results. This local result must not be used to fill the paper's formal RQ3 table or compute Wilcoxon, Bonferroni or effect-size claims.
