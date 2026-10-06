# Review PR #32 — E1 local CPU benchmark

**Ngày review:** 04/10/2026

**Revision:** `c0e5dfa08a3ae29dfb48a205a5a776b83fd10ce3`, [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32). PR đang open tại thời điểm review và chỉ thêm `report.md`; bản report bên cạnh được giữ nguyên văn. Không merge hoặc rerun benchmark trong lần review này.

## Follow-up ngày 06/10/2026 — issue #36

Tác giả đã khôi phục bundle gốc tại `Paper/benchmark-runs/e1-local-20260929T154615Z-9MbzHC/` và tạo `e1-local-20260929-reviewer-bundle.zip` ngoài Git. ZIP local có SHA-256 `1098672d6caf05c758b6a8464cd8d48a867ab561472d9f4ac2740705229578f0`; bên trong có `SHA256SUMS`, raw CSV, log, environment/hardware metadata, lệnh chạy, summary/figure gốc, source snapshot và working-tree diff. [Thư mục evidence trên Drive](https://drive.google.com/drive/folders/1fj6IRxBNfxBgtVineqXHoWaR2CKlU21a?usp=sharing) đã được cung cấp với các file đã tách ra và mở được bằng tài khoản UIT của tác giả. **Reviewer vẫn cần tự xác nhận quyền truy cập và đối chiếu checksum các file**, nên P1 chưa được đóng theo acceptance của issue. SHA-256 của ZIP local không phải checksum của một file ZIP trên Drive.

- SHA-256 `raw.csv` khớp report: `7d38725aa46358e49e69deb216616e68bc923a947aa5ae27f7b3e9f048ebe6f7`; `Cargo.lock` khớp `09a4b0bc93cb4988a224a5fc6435e80b043865eef4321489bd8c12fd6390c447`.
- [Kiểm tra độc lập](verification.json) xác nhận 600 record, 20 ô C1–C5 × `{8,16,32,64}`, từng ô có đúng seed 1–30 duy nhất, không thiếu/trùng, không metric không hữu hạn hoặc không dương. `status.json` trong bundle ghi `passed: true`; strict runner chỉ ghi row sau prove và verify thành công. Không có cột status riêng trong strict CSV để kiểm tra từng lần thất bại; `run.log` và `status.json` là evidence mức run.
- Toàn bộ 20 dòng prove/verify median, Q25/Q75 khớp raw sau làm tròn. Proof-size median trong mười ô có phần `.5` đã bị báo cáo cũ cắt xuống số nguyên; [report](report.md) và bảng giảng viên nay ghi median chính xác. Các số prove/verify và raw CSV không đổi. Maximum recorded memory là `0.091934204 GiB`, báo cáo `0.091934 GiB`.
- [summary.csv](summary.csv) được tái tạo byte-for-byte từ analyzer lưu trong `source.tar.gz`; gồm median/Q25/Q75 và mean với CI 95% percentile bootstrap của **mean** (`10,000` resample, NumPy seed `20260521`). [Figure](e1_plots.png) dùng mean theo lot size. Không suy diễn CI từ IQR.
- Khi merge `main` chứa Path B schema mới, analyzer của run ngày 29/09 được giữ riêng tại [`scripts/e1_analyze_20260929.py`](../../../scripts/e1_analyze_20260929.py). `scripts/e1_analyze.py` hiện theo Path B schema mới; launcher ngày 29/09 là bản provenance cho source/diff của run gốc, không phải lệnh chạy benchmark trên HEAD mới.
- Commit đã build [`9611f581dfc0d6be959df84db1155fce2ff9f2e6`](https://github.com/vanthinh26102005/nckh-ise-blockchain/commit/9611f581dfc0d6be959df84db1155fce2ff9f2e6) đã được publish qua branch [`codex/e1-source-20260929`](https://github.com/vanthinh26102005/nckh-ise-blockchain/tree/codex/e1-source-20260929). `source.diff` áp dụng được lên commit này và cho ra đúng `runner.rs` cùng `e1_analyze.py` trong snapshot. Snapshot còn có launcher và test analyzer. Diff thay cách báo Linux peak RSS, analyzer và launcher; proving statement C1–C5 trong snapshot khớp commit gốc. Reviewer access tới bundle vẫn cần xác nhận trước khi nâng trạng thái provenance.
- `environment.json` ghi `rustc 1.97.0-nightly`, Python 3.9.6, NumPy 2.0.2, pandas 2.3.3, matplotlib 3.9.4, M1 Pro/macOS 26.5 và thread settings. `build.log`, `command.txt`, `run.log`, `warmup.csv` ghi build, lệnh đo và warm-up riêng. Prove timing loại template setup/witness generation; `peak_ram_gb` là process-lifetime peak RSS tính bằng GiB dù tên cột có `gb`.
- `e1_metadata.json` được ghi ở bước analysis và có thêm `?? infra/slurm/` trong `git_status` so với `git-status.txt`/`environment.json` chụp trước build. Snapshot là tập source được chọn cho build E1, không chứa thư mục Slurm này. Chúng không tham gia lệnh local CPU; reviewer vẫn nên ghi nhận khác biệt thời điểm capture thay vì coi các status là giống hệt.

PR #35 đã merge ngày 05/10/2026. Follow-up này được chuẩn bị trên branch `docs/e1-local-results-review` từ `main` mới nhất; PR mới cần liên kết #35/#36 vì không thể thêm commit vào PR đã merge. Không dùng kết quả local để đóng issue CPU Slurm #25.

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
