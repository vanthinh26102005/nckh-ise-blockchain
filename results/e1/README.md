# E1 benchmark evidence

**Cập nhật:** 04/10/2026

| Bộ dữ liệu | Phạm vi | Trạng thái |
|---|---|---|
| [Policy-v2 local CPU smoke 06/10/2026](local-cpu-2026-10-06-policy-v2/report.md) | Plonky3 C1–C5 at n=8, one seed, depth 16; SP1 guest execute only | Base 5/5 passed; wrapper incomplete-resource; no real SP1 proof |
| [Local CPU 29/09/2026](local-cpu-2026-09-29/report.md) | Plonky3 base C1–C5, 4 lot size × 30 seed; tác giả báo cáo 600/600 phép đo | Full grid base được báo cáo trong PR #32; chưa tái tính từ raw CSV |
| `quick/`, `strict/`, `full/` | Output do Make targets tạo | Tên thư mục/target không chứng minh một run đã hoàn thành |
| `archive/` | Artifact lịch sử | Không dùng làm kết quả hiện tại |

Báo cáo local được nhập nguyên văn từ [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32), head `c0e5dfa08a3ae29dfb48a205a5a776b83fd10ce3`. Xem [review và giới hạn xác thực](local-cpu-2026-09-29/review.md) trước khi sử dụng số liệu trong paper.

Đây là base-circuit CPU benchmark, không có recursive-wrapper timing, SP1, GPU, Fabric/Anvil hoặc Slurm evidence. Raw CSV, log và source snapshot nằm ngoài Git; cần chia sẻ artifact cho reviewer để đối chiếu checksum, seed coverage và tái tạo thống kê. Không tạo raw CSV giả từ các median trong bảng.
