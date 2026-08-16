# E2 End-to-End Latency Benchmark Report

## Configuration & Scope
- **L1 Mode**: `mock`
- **Seeds**: `30`
- **Total Ingested Events**: `863755`
- **Total Formed Lots**: `35498`
- **Proof Components**: E1 Plonky3 base proofs (C2 Poseidon2 Merkle, C3 Threshold/Time, C4 Actor Auth, C5 Nullifier).
- **Proof Timing Scope**: `proof_start_ms` → `proof_end_ms` uses measured E1 `witness_ms + prove_ms` cached by circuit and lot size; reusable template/setup time is outside the timed proof window.
- **L1 Scope**: L1 confirmation is deterministic simulation. No Anvil/Sepolia RPC transaction is submitted by this benchmark.
- **Disclaimer**: Reported numbers represent **base proof pipeline latency**. Full recursive rollup latency is not claimed as Plonky3 recursive wrapper remains blocked upstream.

## Summary Metrics

| Metric | Latency (ms) | Latency (s) |
|---|---:|---:|
| **Median (P50)** | 13740.00 | 13.740 |
| **P95** | 16145.00 | 16.145 |
| **P99** | 17210.00 | 17.210 |
| **Min** | 12297.00 | 12.297 |
| **Max** | 20992.00 | 20.992 |
| **Mean** | 13925.45 | 13.925 |

## Stage 3 RQ2 Context
This E2 latency profile provides empirical baseline measurements for Paper 01 RQ2 under standard EPCIS workload.
