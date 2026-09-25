# E4 — Scalability & Policy Trade-off Methodology (proposed system)

**Issue:** [#22](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/22) · **RQ4**
**Nhánh:** `feat/` (không dùng worktree) · **Ngày soạn:** 2026-09-23

Tài liệu này chốt phương pháp cho E4 trước khi chạy benchmark chính. Nó mô tả grid
sweep, mô hình tải, quy tắc đo/censoring, và ranh giới split-host. Phần harness +
schema + analysis đã được chuẩn bị trước (xem [Trạng thái chuẩn bị](#7-trạng-thái-chuẩn-bị))
để có thể validate toàn tuyến `plan → run → analyze` bằng mock executor ngay bây giờ;
executor thật được cắm vào sau khi E3 aggregate proof gate pass.

## 1. Câu hỏi nghiên cứu

RQ4: **proposed system** scale tới đâu và policy/batching (shipment size, epoch
length) ảnh hưởng thế nào tới throughput, completion, chi phí proof và chi phí
on-chain? E4 **chỉ sweep proposed system**, không nhân 4 baseline trên toàn grid
(baseline thuộc phạm vi E3).

## 2. Dependency bắt buộc

Benchmark chính chỉ hợp lệ khi E3 aggregate proof gate đã pass toàn chuỗi:

```
Fabric (2 org / 2 peer / 3 Raft orderer) → leaf proofs → SP1 aggregate proof → Anvil receipt thật
```

Trước khi E3 pass, chỉ được chạy `--executor mock` để kiểm thử harness/schema/analysis;
số liệu mock **không phải** bằng chứng E4 và bị analyzer từ chối khi `--require-real`.

## 3. Mô hình tải (Poisson)

- Mỗi producer sinh shipment theo Poisson với kỳ vọng **3,84 event/phút**.
- Tổng tải danh nghĩa của một cell = `3,84 × Np` event/phút.
- Inter-arrival lấy mẫu deterministic theo seed (ChaCha20), tái lập được.
- **Drain:** khi ngừng inject event, phải xử lý cạn queue (drain) rồi mới kết thúc
  cell; latency của phần drain vẫn được tính.

## 4. Grid sweep

Trục: `Np` (số producer) × `shipment` (event/lot) × `epoch` (giây) × `seed`.
Mỗi cell chạy **đúng 4 epoch hoàn chỉnh** (`epochs_target = 4`).

### 4.1 Screening (rộng, rẻ — dò vùng biên)

| Trục | Giá trị |
|---|---|
| `Np` | 125, 250, 500, 1000, 2000 |
| `shipment` | 16, 32, 64, 128 |
| `epoch` (s) | 30, 60, 120, 300 |
| seed/cell | 1 |

→ 5 × 4 × 4 = **80 cell**, 80 run.

### 4.2 Confirmation (hẹp, chắc — thống kê)

| Trục | Giá trị |
|---|---|
| `Np` | 125, 500, 2000 |
| `shipment` | 16, 64, 128 |
| `epoch` (s) | 30, 120, 300 |
| seed/cell | 10 |

→ 3 × 3 × 3 = 27 cell × 10 seed = **270 run**.

Seed được sinh deterministic từ `cell_id` (xem `scripts/e4_plan.py`) để mọi lần
lập plan cho ra cùng danh sách run.

## 5. Quy tắc đo & censoring (bắt buộc, không thương lượng)

- Ngừng inject → **drain hết queue** trước khi chốt số.
- Nếu chạm `wall_time`, cell được đánh dấu **`saturated`**; nếu run bị cắt vì lý do
  hạ tầng khác (crash, timeout RPC…) đánh dấu **`censored`**.
- **Không** loại số liệu censored/saturated, **không** thay bằng success. Analyzer
  giữ chúng trong bảng và tô riêng trên heatmap (hatch), đồng thời báo tỉ lệ censored.
- Cell không đạt đúng 4 epoch hoàn chỉnh → không được coi là `ok`.

## 6. Split-host policy

Preflight bắt buộc trên topology Fabric 2org/2peer/3 Raft orderer + Gateway + Anvil
+ GPU prover, lưu metadata A/B CPU–GPU (leaf 8/64 event, aggregate 2 lot).

Nếu GPU proving **không cùng host** với Fabric/Anvil, phải ghi rõ `split_host: true`
và network latency giữa host trong metadata. Run split-host **không được** dùng làm
E4 chính thức; analyzer từ chối chúng khi `--require-single-host`.

## 7. Đầu ra

- Heatmap **throughput** và **completion rate** (theo mặt cắt của grid).
- Bảng/plot **leaf proving time** + **aggregate proving time**.
- **Audit latency** (Fabric ack → Anvil receipt).
- **Gas** và **calldata bytes** on-chain.
- **Bảng trade-off policy**: shipment size × epoch length ↔ throughput / proving cost / gas.

## 8. Commit vs. không commit

**Commit:** script phân tích, manifest/metadata schema, metadata run, selected
tables/figures, methodology (file này).
**Không commit:** raw JSONL/CSV, Fabric crypto/volumes, proof fixture, Anvil artifacts
(xem `.gitignore`). `paper.tex` chỉ cập nhật khi có dữ liệu đo thật; cell censored
phải được báo minh bạch.

## 9. Handoff với E3 (Hữu Trí)

Dùng đúng aggregate public-values / ABI / manifest của E3; **không** tự định nghĩa
lại format proof hoặc root ở host. Điểm cắm trong harness: `RealE3Executor` trong
`scripts/e4_run.py` (hiện là stub, TODO gọi binary/pipeline E3).

## 10. Trạng thái chuẩn bị

| Thành phần | File | Trạng thái |
|---|---|---|
| Methodology | `docs/reports/e4_methodology.md` | ✅ (file này) |
| Sweep plan generator | `scripts/e4_plan.py` | ✅ |
| Run manifest schema | `schemas/e4_run_manifest.schema.json` | ✅ |
| Cell-result schema | `schemas/e4_cell_result.schema.json` | ✅ |
| Runner harness (mock + seam E3) | `scripts/e4_run.py` | ✅ mock; ⏳ real executor chờ E3 |
| Analysis / figures | `scripts/e4_analyze.py` | ✅ |
| Make targets | `Makefile` (`e4-plan`, `e4-run-mock`, `e4-analyze-*`) | ✅ |

Chạy chính (`RealE3Executor`) bị chặn cho tới khi E3 aggregate proof gate pass.
