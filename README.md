# nckh-ise-blockchain

MVP benchmark cho E1 trong hướng nghiên cứu "EPCIS-Aware Recursive ZK-Rollup for EUDR Traceability".

## E1 Cryptographic Benchmark

### Setup

```bash
make bootstrap
```

`make bootstrap` cài `rustup` nếu thiếu và pin toolchain `nightly` qua `rust-toolchain.toml`, vì Plonky2 chính thức hiện vẫn cần nightly.

### Smoke test

```bash
make test
make e1-quick
```

`make e1-quick` chạy strict smoke C1-C5 và wrapper recursive với `events/lot = 8`, `seed = 1` và sinh output đúng guide:

- `results/e1_raw.csv`
- `results/e1_table1.csv`
- `results/e1_plots.png`
- `results/e1_metadata.json`
- `results/e1_report.md`

### Full MVP run

```bash
make e1
```

Lệnh này giữ output V2 compatibility ở `results/e1/`. Bản strict theo guide chạy bằng:

```bash
make e1-strict
```

`make e1-strict` chạy `events/lot = 8,16,32,64`, `30 seed/cell`, profile `coffee-default`, cho các circuit:

- `c1_polygon_outside`: chứng minh điểm fixed-point không nằm trong forbidden polygon đã commit; boundary bị xem là forbidden.
- `c2_poseidon_merkle_depth16`: chứng minh certificate membership với Poseidon Merkle root depth 16, 65,536 leaves.
- `c3_threshold_time`: chứng minh KPI readings không vượt ngưỡng và event time monotonic.
- `c4_eddsa_style_proxy`: blocker/proxy rõ ràng do chưa có gadget Ed25519/EdDSA tương thích pinned Plonky2; không claim EdDSA production.
- `c5_poseidon_nullifier_empty_leaf`: chứng minh Poseidon(lot_id, secret) và empty-leaf non-membership trong Merkle tree depth 16.
- `wrapper_recursive_plonky2`: outer Plonky2 proof verify recursive inner proofs C1-C5.

Raw CSV strict theo guide:

```text
circuit,events_per_lot,seed,prove_time_s,verify_time_ms,proof_size_bytes,peak_ram_gb
```

V2 compatibility vẫn có:

```text
seed,events_per_lot,circuit,prove_ms,verify_ms,proof_bytes,peak_rss_mb,status,note
```

V2 giữ các cột cũ và thêm:

```text
circuit_version,build_ms,witness_ms,setup_ms,gate_count,public_inputs,inner_prove_ms
```

### CLI

```bash
cargo run --release -p e1-bench -- \
  --out results/e1_raw.csv \
  --events 8,16,32,64 \
  --seeds 30 \
  --circuits all \
  --profile coffee-default \
  --jobs 1 \
  --strict-output
```

Profiles: `coffee-small`, `coffee-default`, `stress`.

`--include-placeholders` adds legacy comparison circuits:

- `c1_legacy_bbox`
- `c4_legacy_signature_commitment`

Các dòng `proxy` hoặc `placeholder` không dùng làm claim paper-final. Đặc biệt, C4 strict hiện vẫn là blocker/proxy vì chưa tích hợp được Ed25519/EdDSA gadget tương thích Plonky2 pinned.
