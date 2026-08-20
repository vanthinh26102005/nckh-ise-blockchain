# E2 Latency — Ghi chú thí nghiệm

Ngày: 07/08/2026

> Ghi chú lịch sử: các số liệu và mô tả simulation ở đây không phải Fabric acknowledgement-to-Anvil receipt benchmark.

Ghi chú nội bộ nhóm về cách E2 benchmark hoạt động, dùng để tham khảo khi viết paper hoặc giải thích cho thầy.

---

## 1. Cấu trúc code

E2 nằm ở `crates/e2-bench/src/`, gồm 6 file:

- `main.rs` — CLI (clap), chạy từng seed rồi ghi output.
- `pipeline.rs` — logic chính: chạy 1 seed qua toàn bộ pipeline.
- `workload.rs` — sinh Poisson event stream + dồn events thành lots.
- `l1_adapter.rs` — trait L1 submit/confirm, hiện chỉ có `MockL1Adapter`.
- `stats.rs` — tính percentile, ghi CSV/JSON.
- `types.rs` — struct definitions.

## 2. Flow chạy 1 seed

```
generate_poisson_event_stream(seed, λ=480, 60min)
  → accumulate_lots(events, seed)
    → for each lot:
        build_template(C2/C3/C4/C5)    ← cache theo (circuit, lot_size)
        prove_and_verify(template, lot) ← timing cache theo (circuit, lot_size)
        l1_adapter.submit_and_confirm() ← mock: +12s
        → ghi 1 LatencyRow cho mỗi event trong lot
```

Proof timing cache là điểm quan trọng: lot đầu tiên mỗi `(circuit, lot_size)` chạy prove thật, các lot sau cùng size dùng timing đã cache. Đây là thiết kế có chủ đích — mục đích đo pipeline behavior, không phải re-prove cùng kích cỡ nhiều lần.

Prover giả lập single-thread tuần tự:
```rust
let proof_start_ms = lot.lot_ready_at_ms.max(current_timeline_ms);
```

## 3. L1 Adapter

Hiện tại tất cả modes đều dùng `MockL1Adapter` với delay cố định:

| Mode | Delay |
|---|---|
| mock | 12s |
| local | 1s |
| anvil | 1s |
| sepolia | 15s |

Không kết nối RPC thật. Khi cần chạy Anvil/Sepolia thật thì phải implement adapter riêng với `eth_sendRawTransaction` + poll receipt.

## 4. Workload

Event stream: Poisson, `ChaCha20Rng` seed từ `seed ^ 0x00E2_E2E2`, inter-arrival = `-ln(1-U)/λ`. Deterministic hoàn toàn.

Lot size: `Normal(24, 8)` reject-sample cho [8, 64], RNG riêng (`seed ^ 0x00E2_1007`). Lot cuối có thể nhỏ hơn 8.

## 5. Latency breakdown (ước tính từ kết quả)

Với mock mode, median ~13.74s gồm khoảng:
- L1 confirmation: 12s (chiếm ~87%)
- Batching wait + prover queue: ~1.5–1.7s
- Proof generation thật (C2+C3+C4+C5): ~25–30ms

→ Proof generation không phải bottleneck. L1 confirmation chiếm phần lớn. Nếu đổi sang Anvil (1s delay), median sẽ giảm xuống ~1.5–2s.

## 6. Lưu ý khi viết report/paper

Không claim:
- Kết quả Sepolia nếu chưa chạy Sepolia thật
- Full recursive rollup latency nếu wrapper vẫn blocked
- Số `4.1s / 6.8s` từ draft cũ
- Proof size 196B

Có thể claim:
- Base proof pipeline latency đo thật dưới Poisson workload
- Proof timing là measured (từ E1), amortized qua cache
- Pipeline reproducible qua seed

## 7. Output files

```
results/e2/archive/legacy/
├── raw.csv         ← ~863K rows, quá lớn, gitignored
├── summary.csv     ← 1 row summary
├── metadata.json   ← config + disclaimers
├── report.md       ← auto-generated
└── cdf.png         ← CDF + histogram
```
