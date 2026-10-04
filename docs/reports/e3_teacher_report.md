# Báo cáo E3 — Tổng hợp proof theo epoch và so sánh baseline

**Ngày cập nhật:** 04/10/2026

**Mục tiêu nghiên cứu (RQ3):** Đề xuất tổng hợp proof theo epoch đánh đổi throughput, audit latency và chi phí on-chain thế nào so với các baseline?

**Trạng thái:** Đã có implementation cho recursive gate, bốn mode và analyzer; chưa có bộ kết quả benchmark chính thức trong checkout hiện tại.

## 1. E3 giải quyết phần nào của đề tài

E1 chứng minh một lô EPCIS thỏa C1–C5, còn E2 kiểm tra đường dữ liệu từ Fabric đến proof verification trên Anvil. E3 mở rộng sang nhiều lô trong một epoch: kiểm chứng các proof con, nối các chuyển trạng thái nullifier và tạo một proof tổng hợp để anchor một lần cho epoch đó. Thí nghiệm sau đó so sánh proposed system với ba cách xử lý cùng workload.

Recursion của đường deployment dùng **SP1**, tách biệt với recursive wrapper Plonky3 của E1. Cả hai phục vụ cùng bài toán, nhưng không được báo cáo như một proving system duy nhất.

## 2. Recursive epoch gate đã được triển khai

```text
Fabric commit và ledger readback
    → SP1 compressed policy proofs cho từng leaf
    → epoch guest kiểm chứng proof con và chuỗi old/new nullifier root
    → SP1 aggregate Groth16 proof
    → EpochAggregateAnchor kiểm chứng và cập nhật root trên Anvil
```

Epoch guest ràng buộc các proof con vào cùng epoch, yêu cầu chuỗi root liên tục, commitment con không trùng và số lot/event hợp lệ. Mỗi node nhận tối đa tám proof con; epoch lớn được tổng hợp qua nhiều tầng recursion. Shipment 128 event được chia thành hai leaf 64 event để giữ giới hạn của policy guest.

Public values chứa epoch ID, old/new root, epoch commitment, lot count, event count và validity, cộng thêm **hai digest của verification key**. Hai digest này bind leaf/aggregate program vào contract; chúng không phải hai chỉ số nghiên cứu mới. Contract từ chối sai root/key và anchor lặp cùng epoch.

Gate tối thiểu được thiết kế để ghi hai lot, mỗi lot tám event, vào Fabric; tạo và verify proof leaf/aggregate; kiểm tra tampered child; rồi lấy receipt Anvil thành công với root đúng. Code gate đã có, nhưng **chưa có `results/e3/gate_manifest.json` hoặc receipt của gate trong checkout để xác nhận một lần chạy đã pass**.

## 3. Bốn mode so sánh

| Mode | Đường thực thi | Cách xử lý L1 |
|---|---|---|
| Fabric-only | Commit/query EPCIS trên Fabric | Không có L1 transaction; gas/calldata là N/A |
| Hash-on-chain | Fabric và SHA-256 digest của shipment | Một receipt Anvil cho mỗi shipment |
| VeCroToken-adapted | Fabric, SP1 policy leaf proof và token anchor | Verify/mint trên Anvil theo leaf |
| Proposed | Fabric, compressed leaf proofs và recursive epoch proof | Một receipt Anvil cho mỗi epoch |

**VeCroToken-adapted là bản chuyển thể cho artifact này.** Nó dùng policy C1–C5 và SP1 của nhóm, không tái tạo đầy đủ giao thức cross-chain, attribute encryption hay token protocol gốc của VeCroToken. Paper phải giữ nhãn *adapted* và giới hạn phạm vi so sánh.

Thiết kế E3 cố định 125 producer, tổng tải kỳ vọng 480 event/phút, Poisson arrivals, 60 phút inject, shipment size 64 và epoch 60 giây. Bốn mode dùng cùng lịch arrivals theo từng seed, với ID riêng để tránh tái sử dụng acknowledgement từ mode khác. Bộ chính thức cần **4 mode × 30 paired seed = 120 run**; mỗi run phải hoàn thành 60 epoch và xử lý hết queue, nên elapsed time có thể vượt thời gian inject.

Protocol yêu cầu xoay thứ tự bốn mode theo seed và ghi thứ tự chạy để giảm sai lệch do host/ledger tăng trưởng. `e3_run.py` chạy một mode/seed mỗi lần; người triển khai lịch benchmark cần thực hiện và lưu thứ tự đó. Một topology Fabric/Anvil dùng chung chỉ chạy một run tại một thời điểm vì runner reset anchor state.

## 4. Đo lường và phân tích

Runner lưu throughput, audit latency, gas, calldata và transaction receipts. Với mode có L1, audit clock bắt đầu từ Fabric acknowledgement và kết thúc ở Anvil receipt. Fabric-only chỉ đo khả năng truy cập cục bộ, không được gọi là L1 settlement latency. Gas/calldata được phân bổ theo shipment đã hoàn thành.

Analyzer chỉ chấp nhận so sánh chính thức khi đủ seed 0–29 cho cả bốn mode, cùng Git commit, gate receipt và prover, không có run censored/saturated, đồng thời đủ epoch, shipment và receipt. Output gồm median, IQR, p95 và 95% bootstrap CI của mean với 10.000 lần resample. Sáu cặp throughput được kiểm tra bằng exact two-sided Wilcoxon signed-rank, hiệu chỉnh Bonferroni; Cliff's delta là effect size mô tả. `--allow-partial` phục vụ pilot và không xuất formal p-value khi bộ dữ liệu chưa đạt điều kiện.

## 5. Trạng thái kiểm chứng hiện tại

| Thành phần | Evidence hiện có | Phần còn thiếu |
|---|---|---|
| Epoch guest và aggregate anchor | Source code cho verify child proof, root chain, key binding và anti-replay | Một lần gate chạy thật kèm proof fixture, manifest và receipt |
| Bốn baseline | Rust runner `e4_real_cell`, Python orchestrator và các anchor tương ứng | Bộ 120 run có kiểm soát trên hạ tầng được cấp |
| Analyzer | 5 test Python pass ngày 04/10/2026 | Phân tích trên raw result thực |
| Kết luận RQ3 | Methodology và tiêu chí nhận dữ liệu đã có | Bảng so sánh và kết luận từ số đo |

Kiểm tra mới chạy:

```bash
python3 scripts/test_e3.py
```

Năm test kiểm tra thống kê, từ chối bộ thiếu và ma trận synthetic đủ 120 record. Chúng **không chạy Fabric, SP1 prover hoặc Anvil**; không dùng dữ liệu synthetic này làm benchmark paper. Báo cáo này cũng không khẳng định toàn bộ `make e3-check` đã được chạy trong lần cập nhật tài liệu.

## 6. Các bước để đóng E3

1. Cấp phát GPU qua Slurm và xác nhận môi trường chạy được Fabric Gateway, Anvil và SP1 CUDA.
2. Chạy recursive gate trên đúng source revision, lưu proof/receipt ngoài Git và metadata đối chiếu được root/transaction hash.
3. Chạy 120 mode/seed, giữ workload và ghi run order; tách topology nếu cần chạy song song.
4. Lưu và báo cáo cả run lỗi/hết thời gian. Khi bộ dữ liệu chưa đạt tiêu chí formal, dùng báo cáo pilot và xử lý nguyên nhân trước khi bổ sung run mới.
5. Chạy analyzer, rồi cập nhật bảng RQ3 trong paper bằng kết quả được xác thực.

Nguồn trong repo: [recursive gate](e3_recursive_gate.md), [methodology](e3_methodology.md), [runner](../../scripts/e3_run.py), [analyzer](../../scripts/e3_analyze.py), [epoch guest](../../crates/sp1-e2e/epoch-program/src/main.rs), [aggregate anchor](../../contracts/EpochAggregateAnchor.sol).
