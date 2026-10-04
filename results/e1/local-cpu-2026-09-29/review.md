# Review PR #32 — E1 local CPU benchmark

**Ngày review:** 04/10/2026

**Revision:** `c0e5dfa08a3ae29dfb48a205a5a776b83fd10ce3`, [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32). PR đang open tại thời điểm review và chỉ thêm `report.md`; bản report bên cạnh được giữ nguyên văn. Không merge hoặc rerun benchmark trong lần review này.

## Kết luận và findings

Đây là **báo cáo full grid base E1 trên local CPU**: 5 circuit × 4 kích thước lô × 30 seed = 600 phép đo được tác giả báo cáo thành công. Chưa đủ evidence để reviewer xác nhận độc lập 600/600 hoặc gọi toàn bộ RQ1 đã hoàn tất.

1. **[P1] Cần cung cấp raw evidence để xác nhận 600/600 và bảng thống kê** — [report.md dòng 13](report.md#run-configuration-and-provenance). PR chỉ ghi hash và đường dẫn `Paper/benchmark-runs/e1-local-20260929T154615Z-9MbzHC/` trên máy người chạy. Checkout và PR không có raw CSV, metadata đầy đủ hoặc liên kết artifact mà reviewer truy cập được. Vì vậy chưa kiểm tra được seed 1–30 duy nhất cho mỗi ô, không có record thiếu/trùng, checksum raw, hay tái tính median/IQR/proof size/RAM. Chia sẻ bundle ngoài Git với raw CSV, log, metadata, analyzer output và checksum; không cần commit raw/proof lớn vào repo.
2. **[P2] Source đã chạy chưa tái tạo được từ remote** — [report.md dòng 10](report.md#run-configuration-and-provenance). `git cat-file` không tìm thấy commit `9611f581dfc0d6be959df84db1155fce2ff9f2e6`; GitHub commit API cũng trả 422 “No commit found for SHA”. Report còn ghi có local changes ở launcher/analyzer/Linux memory reporting, nhưng snapshot/diff chưa được cung cấp. Cần publish/correct source commit hoặc cung cấp source snapshot + diff + exact command/environment, để xác nhận build và cách tổng hợp bảng. Không thay source SHA bằng PR documentation SHA nếu đó không phải revision đã build.

Hai điểm này là giới hạn evidence hiện tại, không phải kết luận rằng số đo sai. Không ghi “đã xác thực full E1” hoặc đóng [issue #25](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/25) từ PR này: #25 yêu cầu CPU Slurm evidence riêng.

## Những phần đã kiểm tra

- PR chỉ thay đổi một report; source E1/analyzer/lockfile ở PR head giống checkout đang review.
- Bảng có đúng 20 ô, đủ C1–C5 × `{8,16,32,64}`, không lặp circuit/size; `20 × 30 = 600` chỉ là kiểm tra số ô và claim seed, chưa là xác nhận raw row count.
- Trong mọi ô, `Q25 <= median <= Q75`, metric/proof size dương và đơn vị prove giây / verify mili giây nhất quán.
- SHA-256 `Cargo.lock` ở PR head và checkout cùng là `09a4b0bc93cb4988a224a5fc6435e80b043865eef4321489bd8c12fd6390c447`, khớp report.
- Runner strict chỉ xuất row sau prove/verify thành công; `--circuits all` chọn C1–C5, không tự bao gồm wrapper. Setup và witness time được đo riêng với prove time.
- macOS memory measurement là `getrusage(RUSAGE_SELF).ru_maxrss` chia theo GiB. Giá trị `0.091934 GiB` được report mô tả đúng là peak RSS của process lifetime, không phải RAM riêng cho từng circuit/seed.

Không chạy lại 600 phép đo, không tái tính thống kê từ raw và không suy diễn confidence interval từ IQR. CI và figure cần được tạo từ bundle gốc sau khi artifact được chia sẻ.

## Phạm vi statement và cách đọc bảng

| Circuit | Statement hiện được code đo | Ý nghĩa khi tăng events/lot |
|---|---|---|
| C1 | Geofence cho các event của lot | Chi phí proof có thể tăng theo số event |
| C2 | Một membership path ở depth 16; event được chọn bằng `seed % events` | Không phải một proof cho tất cả certificate/event của lot |
| C3 | Threshold và thời gian tăng trên dãy event | Chi phí proof có thể tăng theo số event |
| C4 | Một Poseidon2 actor authorization statement | Không phải Ed25519 AIR hay chữ ký cho từng event |
| C5 | Một nullifier-map update cho lot, depth 32 | Không phải N lần update theo N event |

Do C2/C4/C5 có statement kích thước cố định, thời gian gần như hằng khi tăng events/lot phù hợp với cấu trúc code. Không cộng median năm circuit thành “median full policy” hoặc diễn giải bảng thành benchmark aggregate/recursive proof. C5 khoảng 2,17 MB là serialized **base STARK**, không so trực tiếp với SP1 Groth16 356 byte hay recursive output chưa đo.

## Điều kiện nâng trạng thái thành dữ liệu đã xác thực

1. Reviewer truy cập được bundle và raw hash khớp giá trị trong report.
2. Có đúng 600 record, 20 ô, mỗi ô đúng seed 1–30 duy nhất; metric hữu hạn/dương, không record thất bại bị loại im lặng.
3. Tái tính median, Q25/Q75, proof median và process memory peak từ raw; đối chiếu rounding với toàn bộ 20 dòng.
4. Source snapshot/diff và command build/run/analyze tái tạo được; xác nhận setup/witness/warm-up exclusion và RAM units.
5. Xuất summary/CI/figure từ artifact gốc, cập nhật RQ1 với giới hạn base CPU này. Slurm, recursion và SP1 measurements tiếp tục được ghi riêng.
