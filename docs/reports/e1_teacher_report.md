# Báo cáo E1 — Chính sách tuân thủ và đường chứng minh ZK

**Ngày cập nhật:** 04/10/2026
**Mục tiêu nghiên cứu (RQ1):** Chi phí mật mã thay đổi thế nào theo kích thước lô EPCIS?

## 1. E1 giải quyết phần nào của đề tài

Một doanh nghiệp cần chứng minh một lô hàng tuân thủ EUDR mà không công khai toàn bộ tọa độ, chứng chỉ, danh tính tác nhân và lịch sử sự kiện. E1 biến yêu cầu đó thành một mệnh đề có thể chứng minh bằng không tiết lộ (zero knowledge): dữ liệu gốc là private witness, còn commitment/root và kết quả tuân thủ là public input.

Mô hình dữ liệu dùng chung là `EpcisEventV1`: bản ghi EPCIS có mã hoá nhị phân cố định, big-endian, gồm version, event/lot/epoch ID, thời gian, readings, tọa độ WGS-84 microdegree có dấu, certificate ID, role và public key. Vì byte representation là canonical, cùng một event luôn cho cùng digest/commitment ở Fabric, prover và verifier.

## 2. Các ràng buộc đã triển khai

| Ràng buộc | Ý nghĩa nghiệp vụ | Cơ chế hiện có |
|---|---|---|
| C1 | Điểm của event nằm **ngoài** vùng cấm EUDR | Point-in-polygon số nguyên WGS-84; polygon đơn giản 3–32 đỉnh, từ chối tự cắt, điểm trên cạnh/đỉnh |
| C2 | Certificate/actor thuộc registry được phép | Poseidon2 Merkle membership |
| C3 | Reading vượt ngưỡng và thời gian tăng chặt | So sánh số nguyên trong AIR |
| C4 | Tác nhân được uỷ quyền cho event | Baseline Plonky3: Poseidon2 actor authorization; đường E2E SP1: `Ed25519 verify_strict` chạy trong zkVM |
| C5 | Một nullifier không thể được dùng lại | Sparse Merkle map 32-bit, kiểm tra old/new root và empty leaf trước insert |

C1–C5 đều bind dữ liệu private với public commitments/root. Các test phủ các trường hợp vùng trong/ngoài/cạnh, polygon lõm hoặc tự cắt, threshold/time lỗi, registry path/root lỗi và replay nullifier.

## 3. Hai đường chứng minh được giữ tách bạch

| Đường | Vai trò | Điều đã kiểm chứng | Không được suy diễn thành |
|---|---|---|---|
| **Plonky3 baseline** | Artifact nghiên cứu để đo từng circuit và recursion | C1–C5 base proof; recursive wrapper prove/verify thật trong Rust | Ed25519 AIR hay Solidity verifier |
| **SP1 deployment path** | Chứng minh policy C1–C5 để có thể verify trực tiếp trên EVM | Guest Rust được prove bằng SP1 Groth16 và Solidity verifier nhận proof trong smoke E2E | Benchmark E1 hoàn chỉnh 30 seeds |

Điểm quan trọng: C4 của baseline không phải Ed25519 AIR. Chữ ký Ed25519 được kiểm tra thật trong guest SP1; vì guest execution nằm trong proof, kết quả xác minh chữ ký được bind vào proof E2E. Cách này khác với việc tự xây SHA-512/Ed25519 AIR trong Plonky3, nên báo cáo/paper phải gọi đúng tên hai phương án.

## 4. Kết quả local CPU và kiểm tra raw evidence

Đã có [báo cáo local CPU ngày 29/09/2026](../../results/e1/local-cpu-2026-09-29/report.md) từ [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32). Ngày 06/10, raw CSV gốc được kiểm tra lại: **600/600 phép đo base C1–C5**, gồm 5 circuit × 4 lot size `{8,16,32,64}` × đúng 30 seed duy nhất mỗi ô; không có record thiếu/trùng hoặc metric không hợp lệ. Cấu hình là `coffee-default`, `--jobs 1`, `--strict-output`. Máy chạy Apple M1 Pro, 8 logical CPU, 32 GiB RAM, macOS 26.5; không dùng GPU. Setup/witness generation và warm-up nằm ngoài proving time.

Các median dưới đây đã được tính lại từ raw CSV. Mười proof-size median bị cắt phần `.5` trong report cũ đã được sửa theo [summary gốc](../../results/e1/local-cpu-2026-09-29/summary.csv):

| Circuit | Prove 8 event (s) | Prove 64 event (s) | Verify 64 event (ms) | Proof 64 event (byte) |
|---|---:|---:|---:|---:|
| C1 | 0.264216 | 2.135078 | 2.830 | 77,727.5 |
| C2 | 0.006701 | 0.006744 | 1.005 | 33,154 |
| C3 | 0.001075 | 0.007320 | 0.718 | 18,918.5 |
| C4 | 0.002045 | 0.002048 | 0.762 | 23,258 |
| C5 | 0.219932 | 0.219728 | 64.701 | 2,168,689.5 |

Report có median/IQR cho đủ 20 ô, đã [đối chiếu lại từ raw](../../results/e1/local-cpu-2026-09-29/verification.json). Maximum recorded memory là `0.091934204 GiB` (~94,1 MiB), đo process-lifetime peak RSS trên macOS, gồm setup; không phải memory riêng cho từng circuit hoặc seed. [Figure](../../results/e1/local-cpu-2026-09-29/e1_plots.png) và CI 95% trong summary dùng mean theo ô; CI là percentile bootstrap của mean với 10.000 resample, không suy từ IQR.

C1/C3 xử lý dãy event, nhưng C2 chỉ chứng minh một membership path depth 16 cho event được chọn, C4 một actor authorization và C5 một nullifier update của lot depth 32. Vì thế C2/C4/C5 gần như không đổi theo số event là đặc tính statement hiện tại; không được gọi chúng là N certificate/signature/nullifier checks. Bảng này không chứa recursive wrapper hoặc SP1 timing, và không phải measurement của một proof tổng hợp C1–C5.

## 5. Trạng thái xác thực và việc còn lại

Implementation và kiểm chứng logic đã có evidence lịch sử cho canonical event, C1–C5, recursion Rust và policy guest. Số test ghi trong bản báo cáo 11/09 là kết quả tại thời điểm đó; lần review này không chạy lại benchmark hoặc Rust tests.

Ngày 04/10, reviewer chỉ kiểm tra được bảng và lockfile. Ngày 06/10, tác giả khôi phục raw, log, metadata, source snapshot và working-tree diff; raw hash khớp report, 20 ô khớp sau correction `.5`, và analyzer tái tạo `summary.csv` byte-for-byte. [Commit đã build `9611f581dfc0d6be959df84db1155fce2ff9f2e6`](https://github.com/vanthinh26102005/nckh-ise-blockchain/commit/9611f581dfc0d6be959df84db1155fce2ff9f2e6) đã được publish; patch áp dụng được và khớp snapshot. [Folder evidence trên Drive](https://drive.google.com/drive/folders/1fj6IRxBNfxBgtVineqXHoWaR2CKlU21a?usp=sharing) đã được cung cấp với các file đã tách ra; tài khoản UIT của tác giả mở được nhưng **reviewer chưa xác nhận quyền truy cập và checksum**. Vì vậy trạng thái là **đã tự kiểm tra evidence gốc, chờ reviewer kiểm chứng qua folder**, chưa nâng toàn bộ RQ1 thành kết quả hoàn tất. Xem [review chi tiết](../../results/e1/local-cpu-2026-09-29/review.md).

## 6. Kế hoạch để đóng E1

1. Reviewer mở [folder evidence trên Drive](https://drive.google.com/drive/folders/1fj6IRxBNfxBgtVineqXHoWaR2CKlU21a?usp=sharing), xác nhận quyền truy cập rồi đối chiếu `SHA256SUMS` cho các file đã tách ra; xác nhận độc lập raw, source và CI. ZIP local `Paper/benchmark-runs/e1-local-20260929-reviewer-bundle.zip` có SHA-256 `1098672d6caf05c758b6a8464cd8d48a867ab561472d9f4ac2740705229578f0` nhưng không thấy file ZIP trong folder Drive.
2. Giữ kiểm tra 600 record/20 ô và `summary.csv`/CI/figure đã tái tạo như evidence base local; không tạo raw từ bảng median.
3. Chốt Plonky3 base CPU là measurement được đưa vào RQ1; giữ recursion/SP1 và toàn pipeline thành phép đo riêng.
4. Chạy CPU Slurm theo issue #25 nếu vẫn là deliverable đã chọn; kết quả local không đóng yêu cầu Slurm. Sau khi evidence được xác thực, cập nhật paper với hardware, timing boundary và giới hạn statement.

**Kết luận:** E1 có full grid base CPU local với raw và source đã tự kiểm tra. Reviewer access và review độc lập vẫn chờ; recursive benchmark và CPU Slurm experiment là deliverable riêng.
