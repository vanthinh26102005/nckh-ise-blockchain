# Báo cáo E2 — End-to-End Latency Benchmark

**Người viết**: Nhân
**Ngày**: 07/08/2026

> Số liệu E2 dựa trên benchmark thực tế của Trí (issue #8), chạy trên máy Trí ngày 07/08/2026 bằng lệnh `make e2`. Không sử dụng số estimate hay số từ draft paper cũ.

---

## 1. E2 là gì, khác E1 chỗ nào

E1 đo hiệu năng từng circuit riêng lẻ: prove time, verify time, proof size cho C2/C3/C4/C5. E2 đo thời gian end-to-end cho toàn bộ pipeline — từ lúc một EPCIS event được ingest cho đến khi epoch chứa nó được confirm ở blockchain layer. Đây là metric trả lời RQ2 trong Stage 3.

Nói ngắn gọn: E1 hỏi "mỗi circuit tốn bao nhiêu", E2 hỏi "cả hệ thống tốn bao nhiêu cho mỗi event".

---

## 2. Cấu hình chạy

| Tham số | Giá trị |
|---|---|
| Profile | `full` |
| Lambda (λ) | 480 events/min (Poisson arrival) |
| Duration | 60 min |
| Seeds | 30 |
| Total events | 863,755 |
| Total lots | 35,498 |
| Lot size | N(24, 8) cắt [8, 64] events/lot |
| L1 mode | `mock` |
| Máy chạy | Apple Silicon (aarch64, macOS) |

Event stream sinh deterministic bằng `ChaCha20Rng`. Lot được dồn tự nhiên theo phân phối normal cắt ngưỡng, mô phỏng cách supply chain event dồn thành lô hàng.

---

## 3. Pipeline đo latency

Mỗi event đi qua các bước sau, mỗi bước ghi 1 timestamp:

1. **Ingestion** (`ingest_at_ms`) — event được sinh và ghi timestamp.
2. **Lot batching** (`lot_ready_at_ms`) — events dồn vào lot, lot sẵn sàng khi event cuối trong lot đến.
3. **Proof generation** (`proof_start_ms` → `proof_end_ms`) — chạy C2→C3→C4→C5 tuần tự cho lot đó. Proof timing dùng `witness_ms + prove_ms` đo được từ E1, cache theo circuit + lot size (xem mục 5.2).
4. **L1 submit** (`submit_tx_at_ms`) — ngay sau proof xong.
5. **L1 confirmation** (`confirmed_at_ms`) — L1 confirm. Mock mode cộng thêm 12,000ms.

Total latency = `confirmed_at_ms − ingest_at_ms`.

---

## 4. Kết quả

Từ `results/e2/summary.csv`:

| Metric | Latency (ms) | Latency (s) |
|---|---:|---:|
| Median (P50) | 13,740 | 13.74 |
| P95 | 16,145 | 16.15 |
| P99 | 17,210 | 17.21 |
| Min | 12,297 | 12.30 |
| Max | 20,992 | 20.99 |
| Mean | 13,925 | 13.93 |

Phần lớn events hoàn tất trong khoảng 13–14 giây. Giá trị median gần sát L1 mock delay (12s) vì proof generation chỉ tốn ~25–30ms tổng cho 4 circuits, phần còn lại là batching wait. P95/P99 không quá xa median, pipeline khá ổn định dưới workload này.

Min ~12.3s xảy ra khi event rơi đúng lúc lot gần đủ và prover đang rỗi. Max ~21s xảy ra khi event phải chờ lot tích lũy lâu + prover bận lot trước.

Biểu đồ CDF và histogram: `results/e2/cdf.png`.

---

## 5. Giới hạn cần biết

### 5.1. L1 mode là `mock`, không phải Sepolia

Tất cả L1 mode trong crate hiện tại đều là simulation — `MockL1Adapter` cộng thêm delay cố định (mock = 12s, anvil = 1s, sepolia = 15s). Không gửi RPC transaction thật lên Anvil hay Sepolia. Kết quả này không phải kết quả Sepolia.

### 5.2. Proof timing dùng cache từ E1

Pipeline gọi `prove_and_verify` thật cho lot đầu tiên mỗi `(circuit, lot_size)`, sau đó cache lại `witness_ms + prove_ms` cho các lot cùng kích cỡ. Số liệu proof là đo thật từ Plonky3, nhưng không chạy lại proof cho mỗi lot — amortize qua cache. Template/setup time không tính vào proof window.

### 5.3. Không chạy recursive wrapper

E2 chỉ chạy base proofs C2–C5. Plonky3-recursion rev `524665d` vẫn panic (`trace_next is always present`) trong aggregation path. Không emit mocked proof. Kết quả E2 là base proof pipeline latency, không phải full recursive rollup latency.

### 5.4. C4 vẫn là Poseidon2 proxy

Giống E1, C4 là Poseidon2 actor authorization proof, không phải EdDSA/Ed25519 thật.

---

## 6. Bước tiếp theo

1. Kết nối L1 thật — chạy Anvil local rồi Sepolia testnet để có confirmation latency thực.
2. Theo dõi upstream Plonky3-recursion để unblock recursive wrapper.
3. Tích hợp kết quả E2 vào draft paper cho phần RQ2.

---

## 7. Tái tạo kết quả

```bash
make e2          # full run: 60 min, 30 seeds
make e2-quick    # smoke test: 5 min, 3 seeds
```

Nếu đã có raw CSV, chỉ cần sinh lại plot + report:
```bash
python3 scripts/e2_analyze.py --raw results/e2/raw.csv --out-dir results/e2 \
  --cdf-out results/e2/cdf.png --report-out results/e2/report.md
```

---

## 8. Tham chiếu

| Tài liệu | Đường dẫn |
|---|---|
| E2 summary CSV | `results/e2/summary.csv` |
| E2 metadata | `results/e2/metadata.json` |
| E2 CDF plot | `results/e2/cdf.png` |
| E2 auto-generated report | `results/e2/report.md` |
| E2 bench crate | `crates/e2-bench/` |
| E1 teacher report | `reports/e1_teacher_report.md` |
| E1 tuần 2 report | `E1-Ngo_Van_Thinh/docs/06-report-to-teacher-week2.md` |
