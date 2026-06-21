# E1 Strict Benchmark Report

- Rows: 48
- Circuits: c2_poseidon2_merkle_depth16, c3_threshold_time, c4_poseidon2_actor_authorization, c5_poseidon2_nullifier_empty_leaf
- Proof size is measured from Plonky3 STARK proofs. The 196B target is not claimed.
- C4 is a Poseidon2 actor authorization proof, not Ed25519/EdDSA production verification.
- Ed25519/EdDSA remains a separate blocker.
- Recursive aggregation uses pinned Plonky3-recursion as a research prototype dependency; current wrapper blocker is upstream panic `trace_next is always present`, and no mocked proof is emitted.

| Circuit | Rows | Mean prove s | Mean verify ms | Mean proof bytes | Target gap bytes |
|---|---:|---:|---:|---:|---:|
| c2_poseidon2_merkle_depth16 | 12 | 0.011926 | 15.522 | 275599.7 | 275403.7 |
| c3_threshold_time | 12 | 0.004536 | 0.817 | 14910.6 | 14714.6 |
| c4_poseidon2_actor_authorization | 12 | 0.000492 | 0.498 | 11717.5 | 11521.5 |
| c5_poseidon2_nullifier_empty_leaf | 12 | 0.015857 | 14.573 | 276152.4 | 275956.4 |
