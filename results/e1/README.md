# E1 benchmark evidence

**Cập nhật:** 08/10/2026

| Bộ dữ liệu | Phạm vi | Trạng thái |
|---|---|---|
| [Local CPU policy v2, 08/10/2026](local-cpu-2026-10-08-policy-v2/report.md) | C1–C5 × 4 lot size × 30 seed × credential depth 6/16; recursive wrapper n=8/64 một seed, depth 16 | Raw CSV mới và strict analyzer có trong repo; local Path B đã xác thực, formal E1 và GPU còn chờ |
| [Local CPU 29/09/2026](local-cpu-2026-09-29/report.md) | Plonky3 base C1–C5, 4 lot size × 30 seed; tác giả báo cáo 600/600 phép đo | Full grid base được báo cáo trong PR #32; chưa tái tính từ raw CSV |
| `quick/`, `strict/`, `full/` | Output do Make targets tạo | Tên thư mục/target không chứng minh một run đã hoàn thành |
| `archive/` | Artifact lịch sử | Không dùng làm kết quả hiện tại |

Báo cáo local được nhập nguyên văn từ [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32), head `c0e5dfa08a3ae29dfb48a205a5a776b83fd10ce3`. Xem [review và giới hạn xác thực](local-cpu-2026-09-29/review.md) trước khi sử dụng số liệu trong paper.

Báo cáo 29/09 là base-circuit CPU benchmark, không có recursive-wrapper timing, SP1, GPU, Fabric/Anvil hoặc Slurm evidence. Raw CSV, log và source snapshot của lượt cũ nằm ngoài Git; cần chia sẻ artifact cho reviewer để đối chiếu checksum, seed coverage và tái tạo thống kê. Không tạo raw CSV giả từ các median trong bảng. Bộ dữ liệu policy v2 ngày 08/10 là lượt chạy mới từ source riêng, không thay thế raw hay provenance còn thiếu của lượt cũ.
