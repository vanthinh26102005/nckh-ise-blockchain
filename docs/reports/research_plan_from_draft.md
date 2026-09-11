# Kế hoạch nghiên cứu được phát triển từ draft paper

**Ngày cập nhật:** 11/09/2026
**Mục đích:** Tóm tắt cho giảng viên cách nhóm đi từ draft định hướng đến implementation hiện tại và các thí nghiệm còn lại.

## 1. Bài toán nghiên cứu

EUDR yêu cầu truy xuất nguồn gốc, geolocation và bằng chứng chứng chỉ đối với hàng hoá có nguy cơ liên quan phá rừng. Nếu đưa nguyên EPCIS lên public blockchain thì lộ dữ liệu thương mại; nếu chỉ lưu trong Fabric thì bên ngoài không tự kiểm chứng được; còn hash đơn thuần không chứng minh dữ liệu có ý nghĩa tuân thủ.

Hướng của nhóm là tách ba trách nhiệm:

1. **Fabric:** giữ dữ liệu EPCIS canonical, quyền truy cập và audit trail.
2. **ZK policy:** chứng minh lô hàng thỏa C1–C5 mà không công bố witness.
3. **EVM:** chỉ nhận proof hợp lệ rồi anchor epoch/state root để tạo điểm kiểm chứng công khai.

## 2. Kế hoạch thực nghiệm trong draft

| Experiment | Câu hỏi | Thiết kế dự kiến trong draft |
|---|---|---|
| E1 / RQ1 | Chi phí proof thay đổi theo kích thước lô? | `{8,16,32,64}` event/lot, 30 seed; proof/verify time, RAM, proof size |
| E2 / RQ2 | Độ trễ toàn pipeline là bao nhiêu? | 480 event/phút, 60 phút, 30 seed; đo ingest đến L1 confirmation |
| E3 / RQ3 | Đề xuất có đáng đổi trade-off so với baseline? | So sánh Fabric-only, hash-on-chain, VeCroToken và proposed system |
| E4 / RQ4 | Hệ thống scale và policy/batching ảnh hưởng ra sao? | số producer, lot size, epoch duration; 10 seed cho mỗi cấu hình |

Đây là khung câu hỏi nghiên cứu, không phải cam kết phải giữ nguyên mọi thư viện hay topology. Trong quá trình làm, nhóm thay đổi implementation khi cần để bảo đảm mỗi claim có thể được kiểm chứng thật.

## 3. Kế hoạch implementation đã chọn

### P0 — Nền tảng đúng dữ liệu và policy (đã làm)

- Chốt `EpcisEventV1` canonical và fixture dùng chung.
- Triển khai C1 geofence, C2 registry, C3 threshold/time, C4 authorization/signature path, C5 nullifier chống replay.
- Giữ Plonky3 làm baseline nghiên cứu C1–C5 và recursive wrapper Rust.

### P1 — Đường end-to-end có thể kiểm chứng trên EVM (đã làm smoke)

- Dùng guest Rust trong SP1 để thực thi C1–C5, tạo Groth16 proof.
- Dùng verifier Solidity thực và `EpochAnchor` trên Anvil; proof/public input bị sửa phải bị từ chối.
- Nối Fabric acknowledgement, không thay bằng delay giả.

Việc dùng SP1 không thay đổi bài toán hay policy. Nó thay thế hướng tự port verifier Plonky3 recursion sang Solidity — phần có chi phí kỹ thuật rất lớn và không tạo thêm dữ liệu thực nghiệm cho câu hỏi EUDR — bằng một proving system có đường Groth16/EVM đã kiểm chứng.

### P2–P4 — Thí nghiệm để hoàn thành paper (chưa làm)

1. **E1:** benchmark có kiểm soát, 30 seed theo lot size.
2. **E2:** workload đầy đủ, prove lại từng lot, đo Fabric acknowledgement đến Anvil receipt.
3. **E3:** tái tạo các baseline dưới cùng workload rồi so sánh có thống kê.
4. **E4:** sweep producer/lot/epoch, phân tích scalability và policy trade-off.

## 4. Điểm quyết định trước khi chạy benchmark lớn

- **Compute:** smoke CPU đã cho thấy proof chiếm phần lớn latency; cần benchmark CPU–GPU thật nếu sử dụng DGX, không suy diễn tốc độ từ thông số phần cứng.
- **Phạm vi E2:** full workload có khoảng 864 nghìn event/30 seed; cần chốt budget compute và topology mục tiêu trước khi chạy.
- **E3:** cần một implementation/reproduction rõ ràng cho VeCroToken và các baseline để so sánh công bằng.
- **Paper:** chỉ đưa claim/số liệu đã đo; đồng bộ `docs/stages/stage-3/paper.tex` sau khi phương pháp và benchmark được chốt.

## 5. Tiêu chí “hoàn thành paper”

Paper chỉ có thể gọi là hoàn thành về mặt thực nghiệm khi P2–P4 có raw result tái tạo được, phân tích thống kê và paper được đồng bộ. Hiện nay nhóm đã vượt qua bước feasibility: policy và đường Fabric → proof → EVM chạy thật; phần còn lại là biến feasibility đó thành kết quả thực nghiệm đáng báo cáo.
