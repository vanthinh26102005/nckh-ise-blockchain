# Experiment Guide — Paper 01: EPCIS-Aware Recursive ZK-Rollup for EUDR Traceability

> Hướng dẫn thí nghiệm chi tiết cho nhóm thực hiện. Mọi tham số được liệt kê đầy đủ để có thể tái lập 100%. Hạn nộp tạp chí: **31/08/2026**.

## 1. Tổng quan và bản đồ RQ → Thí nghiệm

| RQ | Câu hỏi | Thí nghiệm | Output chính |
|----|---------|-----------|--------------|
| RQ1 | Chi phí mật mã (prove/verify/size/RAM) theo số EPCIS event mỗi lot? | E1 — Cryptographic benchmark | Bảng/đồ thị: thời gian prove, verify, kích thước proof, RAM |
| RQ2 | Latency end-to-end của kiến trúc 3 tầng dưới tải EUDR thực tế? | E2 — End-to-end latency | Phân phối latency (median, p95, p99) |
| RQ3 | So sánh với 3 baseline (Fabric riêng, hash-on-chain, VeCroToken) trên throughput, byte/shipment, audit latency, chi phí USD? | E3 — Baseline comparison | Bảng so sánh đầy đủ + Wilcoxon p-values |
| RQ4 | Khả năng mở rộng theo số producer, lot/shipment, epoch length, hàm ý cho EUDR/CBAM/DPP? | E4 — Scalability & policy | Đồ thị scaling + bảng tổng hợp policy |

## 2. Môi trường

- **Hardware**: Workstation 16-core (AMD Ryzen 9 5950X hoặc tương đương), 32 GB RAM, ổ NVMe SSD ≥ 1 TB (Nếu cần GPU thì vẫn có thể dùng A100 của UIT).
- **Hệ điều hành**: Ubuntu 22.04 LTS.
- **Ngôn ngữ & runtime**: Rust 1.78+ (cho Plonky2), Go 1.22 (cho Fabric chaincode), Python 3.11 (cho phân tích).
- **Thư viện chính**: `plonky2` (rev mainnet stable), `hyperledger-fabric` 2.5, `web3.py` 6.x, `ethers.js` 6.x, `pandas`, `scipy.stats`, `matplotlib`.
- **Blockchain**: Ethereum Sepolia testnet cho L1; mạng Fabric 3 orderer (Raft) cho L0.

## 3. Bộ dữ liệu

- **EPCIS synthesis**: Sinh từ thống kê thương mại cà phê Việt Nam → EU (Tổng cục Hải quan VN 2023–2024). 500 hợp tác xã giả lập ở Tây Nguyên, 3 nhà chế biến, 2 nhà xuất khẩu, 2 hãng vận tải, 1 importer EU.
- **Event arrival**: Poisson với $\lambda \in \{60, 120, 240, 480, 960\}$ event/phút.
- **Lot composition**: trung bình 24 EPCIS event/lot, phân phối truncated-normal $\mathcal{N}(24, 8)$ giới hạn $[8, 64]$.
- **Geofence polygons**: lấy random từ tập 500 đa giác WGS-84 đã được giảm chiều xuống ≤ 32 đỉnh.
- **Certificate set**: Merkle tree depth 16 (≤ 65k chứng chỉ hợp lệ).

## 4. Bốn thí nghiệm

### E1 — Cryptographic benchmark (RQ1)
- **Biến độc lập**: events/lot ∈ {8, 16, 32, 64}.
- **Biến phụ thuộc**: prove time (s), verify time (ms), proof size (B), peak RAM (GB).
- **Seed**: ≥ 30 seed/cell, đo riêng từng sub-circuit C1–C5 và wrapper.
- **Output**: Table 1 trong paper + 4 line plots (events/lot trục X).

### E2 — End-to-end latency (RQ2)
- **Workload**: $\lambda = 480$ event/phút trong 60 phút × 30 seed.
- **Đo**: thời gian từ EPCIS ingestion đến khi epoch proof được confirm trên Sepolia.
- **Báo cáo**: median, p95, p99, histogram + CDF.

### E3 — Baseline comparison (RQ3)
- 4 hệ thống chạy song song trên cùng workload 1 giờ (lambda = 480).
- **Metrics**: throughput (lot/phút), byte/shipment on-chain, audit latency, chi phí USD (gas price Sepolia × 1; gas → mainnet quy đổi ETH 3000 USD).
- **Kiểm định thống kê**: paired Wilcoxon signed-rank, $\alpha = 0.05$, hiệu chỉnh Bonferroni cho 6 cặp so sánh.

### E4 — Scalability and policy (RQ4)
- $N_p in 125, 250, 500, 1000, 2000, lot/shipment ∈ {16, 32, 64, 128}, epoch length ∈ {30s, 60s, 120s, 300s}.
- 3D grid; mỗi cell 10 seed.
- **Output**: heatmap chi phí, line plot prover time, bảng tóm tắt khả năng đáp ứng EUDR/CBAM/DPP.

## 5. Kiểm định thống kê
- Mỗi cell ≥ 30 seed (E1–E3), ≥ 10 seed (E4 do tải lớn).
- Báo cáo mean ± 95% CI (bootstrap 10,000 lần).
- So sánh cặp: Wilcoxon signed-rank, alpha = 0.05$, Bonferroni khi cần.
- Effect size: Cliff's delta cho các so sánh chính (E3).

## 6. Quy trình tái lập
1. `git clone` repo + checkout tag được khai báo trong artifact.
2. `make bootstrap` cài Rust, Go, Python deps.
3. `make e1`, `make e2`, `make e3`, `make e4` chạy 4 thí nghiệm; raw output vào `results/`.
4. `make figs` sinh hình và bảng cho paper từ raw output.
5. Tổng thời gian dự kiến: ~36 giờ wall-clock trên cấu hình tham chiếu.

## 7. Thời hạn submit: 31/08/2026



## 8. Rủi ro và phương án giảm thiểu
- **R1**: Plonky2 prover memory > 64 GB ở N_p = 2000. → Giảm batch size, tăng số epoch.
- **R2**: Sepolia tắc nghẽn ảnh hưởng E2. → Đo trên local Anvil; báo cáo cả 2.
- **R3**: Fabric baseline crash do peer overload. → Tăng số orderer lên 5; cap workload ở lambda = 480.
