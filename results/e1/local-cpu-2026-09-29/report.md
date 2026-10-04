# E1 local CPU results — 29 September 2026

The current Plonky3 base benchmark completed all **600/600** C1–C5 prove-and-verify measurements on a local CPU. This is a local result; the university Slurm CPU run is separate and has not completed.

## Run configuration and provenance

- Workload: `coffee-default`; 8, 16, 32, and 64 EPCIS events per lot; seeds 1–30 for each circuit and size; `--jobs 1`; `--strict-output`.
- Hardware: Apple M1 Pro (8 logical CPUs, 32 GiB system RAM), arm64; macOS 26.5. No GPU was requested or used.
- Rust: `rustc 1.97.0-nightly (e50aa6fba, 2026-05-19)`; Python 3.9.6.
- Recorded source commit: `9611f581dfc0d6be959df84db1155fce2ff9f2e6`. The run also saved a source snapshot and working-tree diff outside Git. The local changes affected the benchmark launcher, analyzer, and Linux memory reporting; the C1–C5 proving logic on macOS was unchanged.
- Build: `cargo build --locked --release -p e1-bench`. One warm-up seed for each size ran in a separate process and was excluded from the measured CSV.
- Timed values exclude reusable template setup and witness generation. The proof size is the serialized Plonky3 STARK proof.
- Raw CSV SHA-256: `7d38725aa46358e49e69deb216616e68bc923a947aa5ae27f7b3e9f048ebe6f7`. Cargo.lock SHA-256: `09a4b0bc93cb4988a224a5fc6435e80b043865eef4321489bd8c12fd6390c447`. The raw CSV, logs, and full environment metadata remain in the local `Paper/benchmark-runs/e1-local-20260929T154615Z-9MbzHC/` directory outside Git.

## Results

Each row contains 30 successful seeds. Parentheses show the 25th–75th percentile range (IQR). Proof size is the median serialized size in bytes.

| Circuit | Events/lot | Prove median (s), IQR | Verify median (ms), IQR | Proof median (bytes) |
|---|---:|---:|---:|---:|
| C1 | 8 | 0.264216 (0.263429–0.265222) | 2.501 (2.483–2.545) | 64,335 |
| C1 | 16 | 0.537173 (0.529643–0.540516) | 2.627 (2.571–2.728) | 68,511 |
| C1 | 32 | 1.070439 (1.062167–1.085110) | 2.716 (2.686–2.762) | 72,969 |
| C1 | 64 | 2.135078 (2.126849–2.148000) | 2.830 (2.809–2.898) | 77,727 |
| C2 | 8 | 0.006701 (0.006553–0.006767) | 1.006 (0.987–1.025) | 33,118 |
| C2 | 16 | 0.006729 (0.006607–0.006852) | 1.009 (0.992–1.035) | 33,127 |
| C2 | 32 | 0.006749 (0.006627–0.006818) | 1.011 (0.998–1.047) | 33,120 |
| C2 | 64 | 0.006744 (0.006626–0.006798) | 1.005 (0.987–1.015) | 33,154 |
| C3 | 8 | 0.001075 (0.001060–0.001094) | 0.489 (0.485–0.501) | 10,962 |
| C3 | 16 | 0.002001 (0.001954–0.002104) | 0.563 (0.554–0.579) | 13,317 |
| C3 | 32 | 0.003799 (0.003742–0.003897) | 0.629 (0.621–0.661) | 15,979 |
| C3 | 64 | 0.007320 (0.007257–0.007440) | 0.718 (0.701–0.725) | 18,918 |
| C4 | 8 | 0.002045 (0.002026–0.002112) | 0.759 (0.753–0.778) | 23,242 |
| C4 | 16 | 0.002095 (0.002031–0.002148) | 0.760 (0.751–0.791) | 23,240 |
| C4 | 32 | 0.002125 (0.002044–0.002251) | 0.773 (0.761–0.797) | 23,229 |
| C4 | 64 | 0.002048 (0.002024–0.002144) | 0.762 (0.755–0.789) | 23,258 |
| C5 | 8 | 0.219932 (0.219302–0.220613) | 64.552 (64.309–64.769) | 2,168,567 |
| C5 | 16 | 0.221586 (0.220474–0.223089) | 64.810 (64.427–65.522) | 2,168,639 |
| C5 | 32 | 0.221427 (0.219961–0.222243) | 64.940 (64.617–66.016) | 2,168,604 |
| C5 | 64 | 0.219728 (0.218808–0.220661) | 64.701 (64.506–65.169) | 2,168,689 |

C1 is the private polygon/geofence statement; C2 is certificate Merkle membership; C3 is threshold/time; C4 is Poseidon2 actor authorization; C5 is sparse-nullifier update. C4 is **not** Ed25519 AIR. Recursive-wrapper timing is not included.

The maximum recorded memory value was **0.091934 GiB** (approximately 94.1 MiB). On macOS this is the process-lifetime peak RSS, including setup; it is not isolated per circuit or seed. The legacy CSV column is named `peak_ram_gb` although its conversion is in GiB.

This report does not use values from `results/e1/archive/` and does not claim that the Slurm experiment has passed.
