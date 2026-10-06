# E1 policy-v2 local CPU rerun

- Recorded: 2026-10-06T22:12:52+07:00
- Source: b6a8cff7499379e2420f116662e61d92299580f8 (clean detached checkout of PR #38)
- Host: macOS 26.5, Apple Silicon arm64, 32 GiB RAM; CPU prover.
- Scope: local smoke, n=8, seed 1, credential depth 16, coffee-small, production FRI 3/33/16. formalComplete=false.

## Gate and result

- make local-check: passed after fetching the separately locked EPCIS mapper/disclosure dependencies. First attempt stopped on an offline Cargo cache miss; no code or lockfile changed.
- Plonky3 C1–C5: passed-local, 5/5 strict rows ok; analyzer strict passed. Watchdog: 2.78s elapsed, 63.1 MiB process-tree peak RSS, no limit hit.
- Plonky3 wrapper: incomplete-resource (swap_growth_limit after 124.56s; 4.09 GiB process-tree peak RSS). No completed CSV row or verified wrapper proof.
- SP1 policy-v2 eight-event mapper → private witness → guest execute: passed-local, public values matched host; 3.27s, 74.7 MiB process-tree peak RSS. proofGenerated=false.
- SP1 compressed leaf/aggregate Groth16 + Anvil receipt: not-run. The wrapper already exhausted the local swap-growth budget; issue #42 remains the integration gate.

## Base proof measurements (one run per circuit)

| Circuit | Prove (s) | Verify (ms) | Proof (bytes) | Reported process-lifetime peak (GiB) |
|---|---:|---:|---:|---:|
| c1_polygon_outside | 0.550795 | 8.197 | 236,965 | 0.0225 |
| c2_poseidon2_merkle | 0.060422 | 3.373 | 122,419 | 0.0231 |
| c3_threshold_time | 0.028730 | 2.152 | 56,992 | 0.0233 |
| c4_poseidon2_actor_authorization | 0.317450 | 2.969 | 100,207 | 0.0234 |
| c5_poseidon2_sparse_nullifier_depth32 | 0.792208 | 205.138 | 7,492,830 | 0.0630 |

- Base raw SHA-256: 11136df58a595a999a97feb5bc83536e5c73b97c90686eab60742df0dfb0daec; 5 data rows, no failed rows.
- With one seed, these are individual observations. The analyzer one-sample CI bounds are not inferential evidence.
- RSS in the CSV is a process-lifetime high-water mark, not independent RAM use by each circuit. The watchdog separately measured the process-tree peak.
- Path B C2/C4/C5 statements remain narrower than SP1 Path A SHA-256/Ed25519/256-bit-nullifier semantics.

## Reproduction and provenance

- Clean detached source commit: b6a8cff7499379e2420f116662e61d92299580f8; Cargo.lock SHA-256: 09a4b0bc93cb4988a224a5fc6435e80b043865eef4321489bd8c12fd6390c447.
- Host: macOS 26.5 / arm64 / M1 Pro / 32 GiB RAM. Rust nightly 1.97.0; SP1 succinct rustc 1.94.0-dev; SP1 SDK 6.4.0.
- Build base: cargo build --locked --release -p e1-bench.
- Run base: target/release/e1-bench --out EXTERNAL/e1-base.csv --events 8 --seeds 1 --circuits all --profile coffee-small --credential-depth 16 --jobs 1 --strict-output.
- Analyze base: scripts/e1_analyze.py with strict expectations for five circuit identifiers, one seed, eight events and depth 16. Analyzer passed.
- Build wrapper: cargo build --locked --release -p e1-bench --features recursion. Run with the same event/seed/profile/depth settings and --circuits wrapper; watchdog stopped it before a result row was flushed.
- Each run was bounded by 1,800 seconds, 16 GiB process-tree RSS and 512 MiB swap growth. Resource reports are linked below. The separate SP1 guest execute used 120 seconds and 4 GiB RSS limits.
- First make local-check attempt stopped at an offline Cargo cache miss for EPCIS mapper. After cargo fetch --locked for mapper and disclosure lockfiles, the full unmodified make local-check passed.
- Raw metrics, full logs and the private v2 witness remain outside Git. The owner retains the bundle under $HOME/Documents/NCKH-local-validation/e1.ByjSvL. Verify the raw SHA-256 above before using the data.
- The September 2026 archive reviewer bundle separately passed SHA256SUMS and contains 600 records in 20 cells with 30 unique seeds each. Its raw hash is 7d38725aa46358e49e69deb216616e68bc923a947aa5ae27f7b3e9f048ebe6f7. Those measurements predate PR #38 and are excluded here.

## Public files

- [metadata.json](metadata.json): source, workload, status and raw checksum.
- [e1-base-budget.json](e1-base-budget.json): base run resource boundary.
- [e1-wrapper-budget.json](e1-wrapper-budget.json): wrapper stop reason and peak.
- [sp1-execute-budget.json](sp1-execute-budget.json): guest execute resource boundary.

## Remaining work

- Retry wrapper only when local memory headroom is sufficient, or move it to the HPC allocation. Preserve production FRI/security parameters and create a fresh output path.
- Run the real policy-v2 SP1 leaf/aggregate Groth16 and Anvil receipt gate tracked in issue #42, then the formal E1 campaigns (base 1,200 rows across depth 6/16; wrapper 40 rows; SP1 CPU/GPU A/B). This smoke must not populate formal paper results.
