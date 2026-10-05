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

## 4. Kết quả local CPU được báo cáo trong PR #32

Đã có [báo cáo local CPU ngày 29/09/2026](../../results/e1/local-cpu-2026-09-29/report.md), nhập nguyên văn từ [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32). Tác giả báo cáo **600/600 phép đo base C1–C5**: 5 circuit × 4 lot size `{8,16,32,64}` × 30 seed, `coffee-default`, `--jobs 1`, `--strict-output`. Máy chạy là Apple M1 Pro, 8 logical CPU, 32 GiB RAM, macOS 26.5; không dùng GPU. Setup/witness generation và warm-up được ghi là loại khỏi proving time.

Các số dưới đây là median **theo report của tác giả**, chưa được reviewer tái tính từ raw CSV:

| Circuit | Prove 8 event (s) | Prove 64 event (s) | Verify 64 event (ms) | Proof 64 event (byte) |
|---|---:|---:|---:|---:|
| C1 | 0.264216 | 2.135078 | 2.830 | 77,727 |
| C2 | 0.006701 | 0.006744 | 1.005 | 33,154 |
| C3 | 0.001075 | 0.007320 | 0.718 | 18,918 |
| C4 | 0.002045 | 0.002048 | 0.762 | 23,258 |
| C5 | 0.219932 | 0.219728 | 64.701 | 2,168,689 |

Report gốc có đủ median/IQR cho 20 ô. Peak memory được báo cáo là `0.091934 GiB` (~94,1 MiB), đo process-lifetime peak RSS trên macOS, gồm setup; không phải memory riêng cho từng circuit hoặc seed.

C1/C3 xử lý dãy event, nhưng C2 chỉ chứng minh một membership path depth 16 cho event được chọn, C4 một actor authorization và C5 một nullifier update của lot depth 32. Vì thế C2/C4/C5 gần như không đổi theo số event là đặc tính statement hiện tại; không được gọi chúng là N certificate/signature/nullifier checks. Bảng này không chứa recursive wrapper hoặc SP1 timing, và không phải measurement của một proof tổng hợp C1–C5.

## 5. Trạng thái xác thực và việc còn lại

Implementation và kiểm chứng logic đã có evidence lịch sử cho canonical event, C1–C5, recursion Rust và policy guest. Số test ghi trong bản báo cáo 11/09 là kết quả tại thời điểm đó; lần review này không chạy lại benchmark hoặc Rust tests.

Ngày 04/10, reviewer xác nhận bảng đúng 20 ô, median nằm trong IQR, và hash `Cargo.lock` khớp repo. Tuy nhiên raw CSV/log/full metadata còn ở máy người chạy và chưa có artifact link truy cập được; source commit `9611f581dfc0d6be959df84db1155fce2ff9f2e6` không resolve được trên remote, source snapshot/local diff cũng chưa được chia sẻ. Do đó trạng thái hiện tại là **full grid base E1 local CPU được báo cáo, chờ xác thực evidence**, thay cho câu cũ “chưa có benchmark 30 seed”. Xem [review chi tiết](../../results/e1/local-cpu-2026-09-29/review.md).

## 6. Kế hoạch để đóng E1

1. Chia sẻ bundle raw CSV, full metadata, log, source snapshot/diff và command cho reviewer; giữ raw ngoài Git và đối chiếu checksum đã ghi.
2. Xác nhận đúng 600 record/20 ô, seed 1–30 không thiếu/trùng, rồi tái tạo median/IQR, summary/CI và figure từ raw. Không tạo raw từ bảng median.
3. Chốt Plonky3 base CPU là measurement được đưa vào RQ1; giữ recursion/SP1 và toàn pipeline thành phép đo riêng.
4. Chạy CPU Slurm theo issue #25 nếu vẫn là deliverable đã chọn; kết quả local không đóng yêu cầu Slurm. Sau khi evidence được xác thực, cập nhật paper với hardware, timing boundary và giới hạn statement.

**Kết luận:** E1 đã có báo cáo full grid base CPU local, nhưng review dữ liệu chưa hoàn tất. Chưa gọi toàn bộ RQ1, recursive benchmark hay CPU Slurm experiment là hoàn thành từ PR #32.
