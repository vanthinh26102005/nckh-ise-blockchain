# E1 benchmark evidence

**Cập nhật:** 06/10/2026

| Bộ dữ liệu | Phạm vi | Trạng thái |
|---|---|---|
| [Local CPU 29/09/2026](local-cpu-2026-09-29/report.md) | Plonky3 base C1–C5, 4 lot size × 30 seed | Raw hash, 600 record/20 ô và bảng median/IQR đã tái kiểm tra; chờ upload bundle và reviewer xác nhận quyền truy cập |
| `quick/`, `strict/`, `full/` | Output do Make targets tạo | Tên thư mục/target không chứng minh một run đã hoàn thành |
| `archive/` | Artifact lịch sử | Không dùng làm kết quả hiện tại |

Báo cáo ban đầu đến từ [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32), head `c0e5dfa08a3ae29dfb48a205a5a776b83fd10ce3`. Follow-up [issue #36](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/36) đã sửa mười proof-size median từng bị bỏ phần `.5`, thêm [summary với CI 95%](local-cpu-2026-09-29/summary.csv), [figure](local-cpu-2026-09-29/e1_plots.png) và [kết quả kiểm tra raw](local-cpu-2026-09-29/verification.json). Xem [review và giới hạn xác thực](local-cpu-2026-09-29/review.md) trước khi sử dụng số liệu trong paper.

Đây là base-circuit CPU benchmark, không có recursive-wrapper timing, SP1, GPU, Fabric/Anvil hoặc Slurm evidence. Raw CSV, log và source snapshot nằm ngoài Git trong `Paper/benchmark-runs/e1-local-20260929-reviewer-bundle.zip` (SHA-256 `1098672d6caf05c758b6a8464cd8d48a867ab561472d9f4ac2740705229578f0`). Cần upload bundle và xác nhận reviewer truy cập được trước khi nâng trạng thái evidence. Không tạo raw CSV giả từ các median trong bảng.
