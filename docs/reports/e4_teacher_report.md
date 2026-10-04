# Báo cáo E4 — Scalability và trade-off giữa policy và batching

**Ngày cập nhật:** 04/10/2026

**Mục tiêu nghiên cứu (RQ4):** Proposed system scale tới đâu khi thay đổi số producer, shipment size và epoch duration?

**Trạng thái:** Đã có sweep plan, executor thật, preflight CPU/GPU, shard merge và analyzer; chưa có kết quả sweep GPU chính thức trong checkout hiện tại.

## 1. Phạm vi E4

E3 so sánh bốn mode dưới một workload cố định. E4 chỉ sweep **proposed system** để quan sát throughput, completion rate, chi phí proving, audit latency, gas và calldata khi thay đổi tải và batching. Không nhân bốn baseline trên toàn grid E4.

Executor thật gọi binary Rust `e4_real_cell`, thực thi Fabric commit → compressed SP1 leaf proofs → recursive epoch proof → Anvil receipt. Nó dùng lại proof/public-values format và anchor của E3. Mock executor chỉ tạo dữ liệu synthetic để kiểm thử harness và analyzer.

## 2. Grid đã chốt và lưu trong repo

Mỗi producer tạo tải Poisson với kỳ vọng 3,84 event/phút; tải danh nghĩa của một cell là `3,84 × Np`. Mỗi run nhắm tới **bốn epoch hoàn chỉnh**, sau đó phải xử lý hết queue trước khi chốt kết quả.

| Phase | Số producer `Np` | Shipment size | Epoch (giây) | Seed/cell | Tổng run |
|---|---|---|---|---:|---:|
| Screening | 125, 250, 500, 1000, 2000 | 16, 32, 64, 128 | 30, 60, 120, 300 | 1 | 80 |
| Confirmation | 125, 500, 2000 | 16, 64, 128 | 30, 120, 300 | 10 | 270 |

Screening có 80 cấu hình; confirmation có 27 cấu hình, mỗi cấu hình mười seed. Tổng cộng **350 run**, với seed deterministic từ cell ID. Confirmation hiện là grid định sẵn trong plan, không phải kết quả lựa chọn tự động từ screening.

Ngày 04/10/2026, `results/e4/plan.csv` đã được đối chiếu với generator và `plan.summary.json`: đúng 80 + 270 run, mỗi run bốn epoch. **Plan là cấu hình thí nghiệm, không phải bằng chứng 350 run đã thực hiện.** Shipment 128 event được chia thành hai policy leaf 64 event theo implementation E3.

## 3. Điều kiện chạy benchmark chính thức

Runner thật yêu cầu E3 aggregate gate đã pass, cùng Git commit với run, cùng compute host với preflight và đủ metadata topology. Preflight A/B chạy leaf 8/64 event và aggregate hai lot trên CPU/CUDA; lượt GPU tái dùng Fabric evidence của lượt CPU để hai prover xử lý cùng witness. Preflight còn thu receipt, Fabric container ID/image, Anvil version và SP1 SDK version.

Topology chính thức là **Fabric 2 org / 2 peer / 3 Raft orderer, Gateway, Anvil và GPU prover trên cùng compute host**. Nếu Fabric/Anvil nằm trên VM hoặc head node còn prover nằm trên GPU node thì đó là split-host pilot; analyzer với `--require-single-host` từ chối dùng nó làm E4 chính thức.

Vì vậy việc đã SSH được vào VM hoặc nhận hướng dẫn HPC H200 chưa xác nhận môi trường E4 sẵn sàng. Cần kiểm tra GPU allocation, khả năng chạy topology trên compute node và preflight SP1 thực trước khi chọn MIG profile hay ước lượng thời gian benchmark.

## 4. Quy tắc ghi nhận thành công và giới hạn tải

| Trạng thái | Ý nghĩa | Cách báo cáo |
|---|---|---|
| `ok` | Đủ bốn epoch, queue đã drain, event/shipment hoàn thành và có receipt | Dùng các metric đo được |
| `saturated` | Chạm giới hạn wall time hoặc không hoàn tất trong budget | Giữ record và số liệu có được; báo giới hạn tải |
| `censored` | Run bị gián đoạn vì lỗi hạ tầng/thực thi | Giữ record, lý do và metric còn biết; thiếu số liệu được ghi N/A |

Không thay run lỗi bằng số synthetic hoặc loại khỏi bảng. Analyzer giữ số lượng censored/saturated và hiển thị chúng riêng trên heatmap. Một phase E4 đầy đủ có thể chứa các run này vì vùng không hoàn tất chính là một phần của bài toán scalability; E3 formal comparison yêu cầu cả 120 run thành công.

## 5. Harness và đầu ra đã triển khai

Mỗi lời gọi runner có thể chọn một `run_id` hoặc `plan-index` để tạo shard gồm `raw.jsonl` và `manifest.json`. Merge kiểm tra coverage của toàn phase, run ID không trùng, cấu hình đúng plan và metadata code/gate/preflight thống nhất. Một topology dùng chung chỉ chạy một shard tại một thời điểm; chạy song song phải tách ledger, anchor state và cổng.

Analyzer với `--require-real --require-single-host` kiểm tra executor, plan coverage, seed/cell, epoch/queue completion và số receipt. Các đầu ra dự kiến sau khi có dữ liệu hợp lệ là `summary.csv`, `policy_tradeoff.csv`, `report.md`, heatmap throughput/completion và proving-time plot. Phần vẽ hình cần `matplotlib`; việc đã có code xuất hình không đồng nghĩa đã có figure thực nghiệm.

Raw result, proof fixture và runtime state được lưu ngoài Git. Repo giữ plan, methodology và sau này giữ metadata/bảng/figure đã xác thực.

## 6. Evidence hiện tại và các bước còn lại

Ngày 04/10/2026, lệnh dưới đây pass **17 test** về plan, thống kê, executor wrapper, merge và validation:

```bash
python3 scripts/test_e4.py
```

Các test có sử dụng synthetic record và mocked subprocess; chúng không đo tốc độ SP1 CUDA hoặc thực thi cả topology. Checkout hiện chỉ có README, plan CSV và plan summary cho E4; chưa có preflight CPU/GPU, phase manifest, summary hoặc figure của một sweep thực. Chưa thể kết luận ngưỡng tải tối đa, cấu hình batching tốt nhất hay mức tăng tốc GPU.

Các bước để đóng E4:

1. Chạy E3 gate và CPU/GPU preflight trên cùng topology/source revision được dùng cho sweep.
2. Chạy 80 screening shard, ghi đủ cả run thành công và run lỗi/hết thời gian; merge và phân tích toàn phase.
3. Chạy 270 confirmation shard theo plan đã chốt; không tự thay grid hoặc seed mà thiếu cập nhật protocol.
4. Xuất bảng/figure, phân tích trade-off và giới hạn tải, rồi cập nhật paper bằng kết quả thực.

Nguồn trong repo: [methodology](e4_methodology.md), [plan summary](../../results/e4/plan.summary.json), [runner](../../scripts/e4_run.py), [preflight](../../scripts/e4_preflight.py), [merge](../../scripts/e4_merge.py), [analyzer](../../scripts/e4_analyze.py).
