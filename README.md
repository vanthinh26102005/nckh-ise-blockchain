# nckh-ise-blockchain

MVP benchmark cho E1 trong hướng nghiên cứu "EPCIS-Aware Recursive ZK-Rollup for EUDR Traceability".

## E1 Cryptographic Benchmark

### Setup

```bash
make bootstrap
```

`make bootstrap` cài `rustup` nếu thiếu. E1 hiện dùng Plonky3 core cho base proofs. Phase 7 đã pin Plonky3-recursion GitHub rev `524665d`, nhưng wrapper recursive vẫn là research blocker vì upstream rev này panic trong aggregation path; không emit mocked proof.

### Smoke test

```bash
make test
make e1-quick
```

`make e1-quick` chạy smoke base Plonky3 C2-C5 với `events/lot = 8`, `seed = 1`:

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

- `c2_poseidon2_merkle_depth16`: chứng minh certificate/event membership MVP với Poseidon2 path depth 16.
- `c3_threshold_time`: chứng minh KPI readings không vượt ngưỡng và event time monotonic.
- `c4_poseidon2_actor_authorization`: chứng minh actor authorization bằng Poseidon2 và private actor secret; không claim EdDSA/Ed25519.
- `c5_poseidon2_nullifier_empty_leaf`: chứng minh Poseidon2(lot_id, secret, tag) và hashed empty-leaf non-membership MVP.

`wrapper_recursive_plonky3` thử chạy Plonky3-recursion thật khi bật `--features recursion`; nếu upstream panic/fail, row ghi blocker minh bạch và không emit mocked recursive proof.

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

`--include-placeholders` adds legacy comparison circuits, nhưng circuit này đã bị disable trong backend Plonky3:

- `c1_legacy_bbox`

Không claim `196B`. C4 hiện là Poseidon2 actor authorization proof; Ed25519/EdDSA production verification vẫn là blocker riêng ngoài scope E1 base Plonky3.
