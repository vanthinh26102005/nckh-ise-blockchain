# E2 End-to-End Latency Benchmark Report

## Configuration & Scope
- **L1 Mode**: `mock`
- **Seeds**: `30`
- **Total Ingested Events**: `863755`
- **Total Formed Lots**: `35498`
- **Proof Components**: E1 Plonky3 base proofs (C2 Poseidon2 Merkle, C3 Threshold/Time, C4 Actor Auth, C5 Nullifier).
- **Proof Timing Scope**: `proof_start_ms` → `proof_end_ms` uses measured E1 `prove_ms` only; witness/setup time is outside the timed proof window.
- **L1 Scope**: L1 confirmation is deterministic simulation. No Anvil/Sepolia RPC transaction is submitted by this benchmark.
- **Disclaimer**: Reported numbers represent **base proof pipeline latency**. Full recursive rollup latency is not claimed as Plonky3 recursive wrapper remains blocked upstream.

## Summary Metrics

| Metric | Latency (ms) | Latency (s) |
|---|---:|---:|
| **Median (P50)** | 13448.00 | 13.448 |
| **P95** | 15853.00 | 15.853 |
| **P99** | 16916.00 | 16.916 |
| **Min** | 12021.00 | 12.021 |
| **Max** | 20709.00 | 20.709 |
| **Mean** | 13633.23 | 13.633 |

## Stage 3 RQ2 Context
This E2 latency profile provides empirical baseline measurements for Paper 01 RQ2 under standard EPCIS workload.
