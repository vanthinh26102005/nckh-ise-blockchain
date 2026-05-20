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

`make e1-quick` chạy C1-C5 và wrapper recursive với `events/lot = 8`, `seed = 1` và sinh:

- `results/e1/raw.csv`
- `results/e1/summary.csv`
- `results/e1/report.md`
- `results/e1/figs/prove_ms.svg`
- `results/e1/figs/verify_ms.svg`
- `results/e1/figs/proof_bytes.svg`
- `results/e1/figs/peak_rss_mb.svg`

### Full MVP run

```bash
make e1
```

Lệnh này chạy `events/lot = 8,16,32,64`, `30 seed/cell`, profile `coffee-default`, cho các circuit:

- `c1_polygon_halfplane`: chứng minh điểm fixed-point nằm trong convex polygon bằng half-plane constraints, public polygon commitment.
- `c2_poseidon_merkle`: chứng minh certificate membership với Poseidon Merkle root.
- `c3_threshold_time`: chứng minh KPI readings không vượt ngưỡng và event time monotonic.
- `c4_schnorr_proxy`: Schnorr-like algebraic signature proxy trong field, không claim EdDSA production.
- `c5_poseidon_nullifier_set`: chứng minh Poseidon nullifier derivation và non-membership trong committed bounded set.
- `wrapper_recursive_plonky2`: outer Plonky2 proof verify recursive inner proofs C1-C5.

Các cột raw CSV cố định theo kế hoạch:

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
  --out results/e1/raw.csv \
  --events 8,16,32,64 \
  --seeds 30 \
  --circuits all \
  --profile coffee-default \
  --jobs 1
```

Profiles: `coffee-small`, `coffee-default`, `stress`.

`--include-placeholders` adds legacy comparison circuits:

- `c1_legacy_bbox`
- `c4_legacy_signature_commitment`

Các dòng `proxy` hoặc `placeholder` trong `note` không dùng làm claim paper-final. Đặc biệt, C4 hiện là Schnorr-like proxy, chưa phải EdDSA batch verification.
