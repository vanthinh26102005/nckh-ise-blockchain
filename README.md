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

`make e1-quick` chạy smoke base Plonky3 C1-C5 với `events/lot = 8`, `seed = 1`:

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
- `c1_polygon_outside`: chứng minh mọi EPCIS event private có toạ độ WGS-84 microdegree nằm ngoài polygon simple private (3–32 đỉnh), với commitment Poseidon2 cho polygon và batch event.
- `c3_threshold_time`: chứng minh KPI readings không vượt ngưỡng và event time monotonic.
- `c4_poseidon2_actor_authorization`: chứng minh actor authorization bằng Poseidon2 và private actor secret; không claim EdDSA/Ed25519.
- `c5_poseidon2_sparse_nullifier_depth32`: chứng minh cập nhật sparse nullifier map 32-bit từ `oldRoot` sang `newRoot`; `oldRoot`, `newRoot`, nullifier và trạng thái là public inputs. Việc lưu trạng thái map xuyên các giao dịch sẽ được nối vào E2/Fabric ở PR4.

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

## E2 End-to-End Latency Benchmark

Đo thời gian từ lúc EPCIS event được ingest đến khi epoch/proof artifact được submit và confirm ở lớp blockchain (RQ2).

### Smoke Run (Quick Profile)

```bash
make e2-quick
```

Chạy quick profile: `lambda = 480 events/min`, `duration = 5 min`, `seeds = 3`, `l1-mode = mock`.

Sinh các file output trong `results/e2/`:
- `results/e2/raw.csv`
- `results/e2/summary.csv`
- `results/e2/metadata.json`
- `results/e2/report.md`
- `results/e2/cdf.png`

### Full Run (Stage 3 Guide Aligned)

```bash
make e2
```

Chạy full workload: `lambda = 480 events/min`, `duration = 60 min`, `seeds = 30`, `l1-mode = mock`.

### CLI Direct Usage

```bash
cargo run --release -p e2-bench -- \
  --profile quick \
  --lambda-events-per-min 480 \
  --duration-min 5 \
  --seeds 3 \
  --l1-mode mock \
  --out results/e2/raw.csv \
  --summary-out results/e2/summary.csv \
  --metadata-out results/e2/metadata.json
```

Full run CLI:

```bash
cargo run --release -p e2-bench -- \
  --profile full \
  --lambda-events-per-min 480 \
  --duration-min 60 \
  --seeds 30 \
  --l1-mode anvil \
  --out results/e2/raw.csv \
  --summary-out results/e2/summary.csv \
  --metadata-out results/e2/metadata.json
```

### Disclaimers & Disclosures

- E2 mang `EpcisEventV1` canonical (86-byte big-endian) trong workload. C1 được prove trên payload của từng lot; C2–C5 vẫn dùng cache timing theo circuit/lot size cho prototype hiện tại, và sẽ bị bỏ trong benchmark thật PR4.
- Report ghi rõ đây là **base proof pipeline latency**, không claim full recursive rollup latency nếu Plonky3 recursive wrapper vẫn đang blocked.
- L1 confirmation hiện là deterministic simulation: `mock` ~12s, `local/anvil` ~1s, `sepolia` ~15s. Chưa submit transaction thật lên Anvil/Sepolia.
