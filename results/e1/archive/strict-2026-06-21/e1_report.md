# E1 Strict Benchmark Report

- Rows: 480
- Circuits: c2_poseidon2_merkle_depth16, c3_threshold_time, c4_poseidon2_actor_authorization, c5_poseidon2_nullifier_empty_leaf
- Proof size is measured from Plonky3 STARK proofs. The 196B target is not claimed.
- C4 is a Poseidon2 actor authorization proof, not Ed25519/EdDSA production verification.
- Ed25519/EdDSA remains a separate blocker.
- Recursive aggregation uses pinned Plonky3-recursion as a research prototype dependency; current wrapper blocker is upstream panic `trace_next is always present`, and no mocked proof is emitted.

| Circuit | Rows | Mean prove s | Mean verify ms | Mean proof bytes | Target gap bytes |
|---|---:|---:|---:|---:|---:|
| c2_poseidon2_merkle_depth16 | 120 | 0.011021 | 10.858 | 275617.5 | 275421.5 |
| c3_threshold_time | 120 | 0.003773 | 0.646 | 14909.3 | 14713.3 |
| c4_poseidon2_actor_authorization | 120 | 0.000402 | 0.433 | 11697.5 | 11501.5 |
| c5_poseidon2_nullifier_empty_leaf | 120 | 0.010536 | 10.606 | 276199.8 | 276003.8 |
