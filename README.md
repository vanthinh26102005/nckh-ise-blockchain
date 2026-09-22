# nckh-ise-blockchain

MVP benchmark cho E1 trong hướng nghiên cứu "EPCIS-Aware Recursive ZK-Rollup for EUDR Traceability".

## Repository Layout

- `crates/`: Rust workspaces for E1 and E2.
- `docs/`: research documents, reports, stage submissions, and references; see `docs/README.md`.
- `results/`: benchmark outputs grouped by experiment and run kind; see `results/README.md`.
- `scripts/`: bootstrap and result-analysis scripts.

## E1 Cryptographic Benchmark

### Setup

```bash
make bootstrap
```

`make bootstrap` cài `rustup` nếu thiếu. E1 dùng Plonky3 core `0.6` cho base proofs và Plonky3-recursion revision `b363397` cho wrapper. Với `--features recursion`, wrapper đã prove/verify thật C1–C5 trong cấu hình Goldilocks/Poseidon2/FRI hiện tại. Đây vẫn là research artifact: C4 bên trong proof là Poseidon2 actor-authorization statement, chưa phải Ed25519 AIR.

### Smoke test

```bash
make test
make e1-quick
```

`make e1-quick` chạy smoke base Plonky3 C1-C5 với `events/lot = 8`, `seed = 1`:

- `results/e1/quick/raw.csv`
- `results/e1/quick/summary.csv`
- `results/e1/quick/plots.png`
- `results/e1/quick/metadata.json`
- `results/e1/quick/report.md`

### Full MVP run

```bash
make e1
```

Lệnh này ghi output đầy đủ vào `results/e1/full/`. Bản strict theo guide chạy bằng:

```bash
make e1-strict
```

`make e1-strict` chạy `events/lot = 8,16,32,64`, `30 seed/cell`, profile `coffee-default`, cho các circuit:

- `c2_poseidon2_merkle_depth16`: chứng minh certificate/event membership MVP với Poseidon2 path depth 16.
- `c1_polygon_outside`: chứng minh mọi EPCIS event private có toạ độ WGS-84 microdegree nằm ngoài polygon simple private (3–32 đỉnh), với commitment Poseidon2 cho polygon và batch event.
- `c3_threshold_time`: chứng minh KPI readings không vượt ngưỡng và event time monotonic.
- `c4_poseidon2_actor_authorization`: chứng minh actor authorization bằng Poseidon2 và private actor secret; không claim EdDSA/Ed25519.
- `c5_poseidon2_sparse_nullifier_depth32`: chứng minh cập nhật sparse nullifier map 32-bit từ `oldRoot` sang `newRoot`; `oldRoot`, `newRoot`, nullifier và trạng thái là public inputs. Việc lưu trạng thái map xuyên các giao dịch sẽ được nối vào E2/Fabric ở PR4.

`wrapper_recursive_plonky3` chạy Plonky3-recursion thật khi bật `--features recursion` và chỉ ghi kết quả khi prove + recursive verification thành công. Không có mocked proof. Cấu hình wire-format Solidity verifier và contract EVM chưa có trong repository.

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

Khi chạy CLI trực tiếp, tạo trước thư mục output:

```bash
mkdir -p results/e1/strict
```

```bash
cargo run --release -p e1-bench -- \
  --out results/e1/strict/raw.csv \
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

Không claim `196B`. C4 hiện là Poseidon2 actor authorization proof; Ed25519/EdDSA production verification là hướng mở rộng ngoài scope E1 artifact hiện tại.

## E2 End-to-End Latency Benchmark

RQ2 đo từ **Fabric commit acknowledgement của event cuối trong lot** đến **Anvil receipt** xác nhận SP1 Groth16 proof. Đường này dùng `EpcisEventV1` canonical, Fabric Gateway/chaincode thật, SP1 verify local và Solidity verifier thật; không có proof-timing cache hay L1 delay giả.

### Real E2 runner

Khởi động topology Fabric 2 org/2 peer/3 Raft orderer và Gateway theo [infra/fabric/README.md](infra/fabric/README.md), sau đó chạy Anvil và anchor service trên **cùng host** với runner:

```bash
anvil --port 8545
cd contracts && npm run anvil:e2-anchor
```

Pilot tăng tốc chỉ dùng để debug, không phải số liệu paper:

```bash
cargo run --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e \
  --bin e2_real_benchmark -- \
  --accelerated --duration-min 1 --seeds 1 \
  --out results/e2/real/raw.jsonl
```

Benchmark chính phải pace theo đồng hồ thực và dùng GPU prover sau khi preflight GPU pass:

```bash
cargo run --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e \
  --features cuda --bin e2_real_benchmark -- \
  --prover cuda --lambda-events-per-min 480 --duration-min 60 --seeds 30 \
  --out results/e2/real/raw.jsonl
```

`raw.jsonl` chứa từng lot: event count, proof time, Anvil anchor time, Fabric-acknowledgement-to-receipt latency, proof bytes, gas và transaction hash. Setup proving key được ghi rõ là ngoài timed window; mỗi lot vẫn tạo/verify một Groth16 proof mới. Raw output không được commit.

Sau một run hoàn chỉnh, kiểm tra đủ seed và tạo summary tái lập được:

```bash
python3 scripts/e2_real_analyze.py \
  --raw results/e2/real/raw.jsonl \
  --out results/e2/real/summary.json \
  --expected-seeds 30
```

Topology benchmark trong repo là 2 org/2 peer/**3 EtcdRaft orderer**. Bộ 60 phút × 30 seed chỉ là số liệu paper sau khi GPU preflight pass và `raw.jsonl` được analyzer kiểm tra đủ 30 seed.

### Legacy simulation (không dùng làm evidence RQ2)

```bash
make e2-quick
```

Chạy quick profile: `lambda = 480 events/min`, `duration = 5 min`, `seeds = 3`, `l1-mode = mock`.

Sinh các file output trong `results/e2/quick/`:
- `results/e2/quick/raw.csv`
- `results/e2/quick/summary.csv`
- `results/e2/quick/metadata.json`
- `results/e2/quick/report.md`
- `results/e2/quick/cdf.png`

`make e2` và `make e2-quick` được giữ để tái lập prototype Plonky3 cũ, nhưng có cache C2–C5 và L1 simulation. Không đưa CSV hay biểu đồ từ các lệnh này vào paper E2.

### Historical CLI

```bash
make e2
```

Chạy full workload: `lambda = 480 events/min`, `duration = 60 min`, `seeds = 30`, `l1-mode = mock`.

```bash
cargo run --release -p e2-bench -- \
  --profile quick \
  --lambda-events-per-min 480 \
  --duration-min 5 \
  --seeds 3 \
  --l1-mode mock \
  --out results/e2/quick/raw.csv \
  --summary-out results/e2/quick/summary.csv \
  --metadata-out results/e2/quick/metadata.json
```

Full run CLI:

```bash
cargo run --release -p e2-bench -- \
  --profile full \
  --lambda-events-per-min 480 \
  --duration-min 60 \
  --seeds 30 \
  --l1-mode anvil \
  --out results/e2/strict/raw.csv \
  --summary-out results/e2/strict/summary.csv \
  --metadata-out results/e2/strict/metadata.json
```

`crates/e2-bench/` không gọi Fabric, SP1 hay RPC Anvil; nó là historical artifact tách biệt với `crates/sp1-e2e/script/src/bin/e2_real_benchmark.rs`.
