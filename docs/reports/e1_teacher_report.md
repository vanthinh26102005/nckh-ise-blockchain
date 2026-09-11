# Báo cáo E1 — Chính sách tuân thủ và đường chứng minh ZK

**Ngày cập nhật:** 11/09/2026
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

## 4. Trạng thái E1 hiện tại

Đã hoàn thành phần **implementation và kiểm chứng logic**: canonical event, C1–C5, recursion Rust cho baseline, và policy guest dùng cho E2E. `cargo test --workspace` đã pass 32 test E1, 7 EPCIS integration test và 7 test E2 tại lần kiểm chứng gần nhất.

Chưa hoàn thành phần **thí nghiệm RQ1** của paper: chạy lặp 30 seeds cho các kích thước lô `{8, 16, 32, 64}`, ghi proof time, verify time, RAM và proof size; sau đó báo cáo median/IQR/CI. Không có số benchmark chưa đo nào được dùng làm kết quả nghiên cứu.

## 5. Kế hoạch để đóng E1

1. Chốt một đường benchmark duy nhất (Plonky3 baseline hoặc SP1 deployment path) và cấu hình máy chạy.
2. Chạy warm-up, sau đó 30 seeds/lô; template/artifact setup được ghi riêng, không lẫn vào proving time.
3. Lưu raw result ngoài Git, tái tạo bảng/biểu đồ và kiểm tra regression.
4. Cập nhật paper chỉ bằng số liệu đã đo, đồng thời nêu rõ đường chứng minh được đo.

**Kết luận:** E1 đã có mệnh đề tuân thủ và proof path hoạt động; chưa được gọi là E1 thực nghiệm hoàn chỉnh cho đến khi benchmark có kiểm soát ở trên hoàn tất.
