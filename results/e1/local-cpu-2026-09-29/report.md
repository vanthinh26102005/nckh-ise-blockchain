# E1 local CPU results — 29 September 2026

The current Plonky3 base benchmark completed all **600/600** C1–C5 prove-and-verify measurements on a local CPU. On 6 October 2026, the original raw CSV passed an independent coverage and table check. The [off-Git evidence folder](https://drive.google.com/drive/folders/1fj6IRxBNfxBgtVineqXHoWaR2CKlU21a?usp=sharing) contains the extracted files; reviewer access and checksum verification are tracked in [the review](review.md). This is a local result; the university Slurm CPU run is separate and has not completed.

## Run configuration and provenance

- Workload: `coffee-default`; 8, 16, 32, and 64 EPCIS events per lot; seeds 1–30 for each circuit and size; `--jobs 1`; `--strict-output`.
- Hardware: Apple M1 Pro (8 logical CPUs, 32 GiB system RAM), arm64; macOS 26.5. No GPU was requested or used.
- Rust: `rustc 1.97.0-nightly (e50aa6fba, 2026-05-19)`; Python 3.9.6.
- Recorded source [commit `9611f581dfc0d6be959df84db1155fce2ff9f2e6`](https://github.com/vanthinh26102005/nckh-ise-blockchain/commit/9611f581dfc0d6be959df84db1155fce2ff9f2e6), now reachable through [the provenance ref](https://github.com/vanthinh26102005/nckh-ise-blockchain/tree/codex/e1-source-20260929). The run saved a 54-file source subset and working-tree diff outside Git. Applying the diff to the recorded commit reproduces the modified runner and analyzer in the snapshot; the launcher and analyzer test are included as untracked source. The changes affect the benchmark launcher, analyzer, and Linux memory reporting. C1–C5 proving logic on macOS was unchanged.
- Build: `cargo build --locked --release -p e1-bench`. One warm-up seed for each size ran in a separate process and was excluded from the measured CSV.
- Timed values exclude reusable template setup and witness generation. The proof size is the serialized Plonky3 STARK proof.
- Raw CSV SHA-256: `7d38725aa46358e49e69deb216616e68bc923a947aa5ae27f7b3e9f048ebe6f7`. Cargo.lock SHA-256: `09a4b0bc93cb4988a224a5fc6435e80b043865eef4321489bd8c12fd6390c447`. The original raw CSV, logs, and full environment metadata remain in `Paper/benchmark-runs/e1-local-20260929T154615Z-9MbzHC/` outside Git. The [Drive folder](https://drive.google.com/drive/folders/1fj6IRxBNfxBgtVineqXHoWaR2CKlU21a?usp=sharing) contains extracted evidence files and per-file `SHA256SUMS`. A separate local reviewer ZIP has SHA-256 `1098672d6caf05c758b6a8464cd8d48a867ab561472d9f4ac2740705229578f0`; this ZIP checksum does not identify a file in the Drive folder.

## Results

Each row contains 30 successful seeds. Parentheses show the 25th–75th percentile range (IQR). Proof size is the exact median serialized size in bytes. A `.5` median is the midpoint of two integer proof sizes, not a fractional proof. The original PR #32 report truncated these ten `.5` medians to whole bytes; the correction below preserves the values in the captured `summary.csv` without changing raw data or prove/verify statistics.

| Circuit | Events/lot | Prove median (s), IQR | Verify median (ms), IQR | Proof median (bytes) |
|---|---:|---:|---:|---:|
| C1 | 8 | 0.264216 (0.263429–0.265222) | 2.501 (2.483–2.545) | 64,335.5 |
| C1 | 16 | 0.537173 (0.529643–0.540516) | 2.627 (2.571–2.728) | 68,511 |
| C1 | 32 | 1.070439 (1.062167–1.085110) | 2.716 (2.686–2.762) | 72,969 |
| C1 | 64 | 2.135078 (2.126849–2.148000) | 2.830 (2.809–2.898) | 77,727.5 |
| C2 | 8 | 0.006701 (0.006553–0.006767) | 1.006 (0.987–1.025) | 33,118 |
| C2 | 16 | 0.006729 (0.006607–0.006852) | 1.009 (0.992–1.035) | 33,127.5 |
| C2 | 32 | 0.006749 (0.006627–0.006818) | 1.011 (0.998–1.047) | 33,120.5 |
| C2 | 64 | 0.006744 (0.006626–0.006798) | 1.005 (0.987–1.015) | 33,154 |
| C3 | 8 | 0.001075 (0.001060–0.001094) | 0.489 (0.485–0.501) | 10,962 |
| C3 | 16 | 0.002001 (0.001954–0.002104) | 0.563 (0.554–0.579) | 13,317 |
| C3 | 32 | 0.003799 (0.003742–0.003897) | 0.629 (0.621–0.661) | 15,979.5 |
| C3 | 64 | 0.007320 (0.007257–0.007440) | 0.718 (0.701–0.725) | 18,918.5 |
| C4 | 8 | 0.002045 (0.002026–0.002112) | 0.759 (0.753–0.778) | 23,242.5 |
| C4 | 16 | 0.002095 (0.002031–0.002148) | 0.760 (0.751–0.791) | 23,240.5 |
| C4 | 32 | 0.002125 (0.002044–0.002251) | 0.773 (0.761–0.797) | 23,229.5 |
| C4 | 64 | 0.002048 (0.002024–0.002144) | 0.762 (0.755–0.789) | 23,258 |
| C5 | 8 | 0.219932 (0.219302–0.220613) | 64.552 (64.309–64.769) | 2,168,567 |
| C5 | 16 | 0.221586 (0.220474–0.223089) | 64.810 (64.427–65.522) | 2,168,639 |
| C5 | 32 | 0.221427 (0.219961–0.222243) | 64.940 (64.617–66.016) | 2,168,604 |
| C5 | 64 | 0.219728 (0.218808–0.220661) | 64.701 (64.506–65.169) | 2,168,689.5 |

C1 is the private polygon/geofence statement; C2 is certificate Merkle membership; C3 is threshold/time; C4 is Poseidon2 actor authorization; C5 is sparse-nullifier update. C4 is **not** Ed25519 AIR. Recursive-wrapper timing is not included.

The maximum recorded memory value was **0.091934 GiB** (approximately 94.1 MiB). On macOS this is the process-lifetime peak RSS, including setup; it is not isolated per circuit or seed. The legacy CSV column is named `peak_ram_gb` although its conversion is in GiB.

## Reproduction and statistical method

- [Verification result](verification.json): raw SHA-256 matches, exactly 600 records form 20 cells with unique seeds 1–30, all numeric metrics are finite and positive, and all 20 report rows match raw median/Q25/Q75 after display rounding. Maximum recorded RSS is `0.091934204 GiB`. `status.json` in the off-Git bundle records successful completion; the strict runner emits a row only after proving and verification succeed.
- [Captured summary](summary.csv): for each cell, median/Q25/Q75 plus arithmetic mean and a 95% percentile bootstrap CI **of the mean**. The captured analyzer uses 10,000 resamples and NumPy RNG seed `20260521`. These confidence intervals are recomputed from raw observations, not inferred from IQR. [Figure](e1_plots.png) plots cell means by lot size; memory values remain process-lifetime peaks.
- To reproduce, extract `source.tar.gz` from the reviewer bundle, then run its `scripts/e1_analyze.py --raw /path/to/raw.csv --out-dir /path/to/recomputed --expected-seeds 30 --expected-events 8,16,32,64 --base-grid`. With NumPy 2.0.2, pandas 2.3.3 and matplotlib 3.9.4, the resulting `summary.csv` matches the captured file byte for byte. The repository also preserves this [historical analyzer](../../../scripts/e1_analyze_20260929.py) and an [independent table check](../../../scripts/e1_verify_local_evidence.py). The current `scripts/e1_analyze.py` serves the newer Path B schema.
- The captured `command.txt` gives the original measured binary invocation; the portable equivalent is `target/release/e1-bench --events 8,16,32,64 --circuits c1,c2,c3,c4,c5 --profile coffee-default --jobs 1 --strict-output --seeds 30 --out raw.csv`. Warm-up used the same flags with one seed and a separate output file. `environment.json` records the exact Rust/Python versions, package versions, thread settings, and hardware metadata.

This report does not use values from `results/e1/archive/` and does not claim that the Slurm experiment has passed.
