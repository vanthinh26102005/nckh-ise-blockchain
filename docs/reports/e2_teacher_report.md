# Báo cáo E2 — Kiểm chứng end-to-end từ Fabric đến Anvil

**Ngày cập nhật:** 11/09/2026
**Mục tiêu nghiên cứu (RQ2):** Từ lúc Fabric xác nhận một lô EPCIS đến khi public chain ghi nhận kết quả tuân thủ mất bao lâu?

## 1. Pipeline đang kiểm chứng

```text
EpcisEventV1 canonical bytes
        │
        ▼
Fabric Gateway ──► Fabric ledger + EventIngested acknowledgement
        │
        ▼
SP1 guest kiểm tra C1–C5 ──► Groth16 proof + public values
        │
        ▼
Anvil: EpochAnchor.verifyAndAnchor ──► epoch/state root
```

Fabric chaincode `e2epcis` lưu canonical bytes và SHA-256 digest, phát `EventIngested`, đồng thời từ chối schema, digest hoặc ID trùng. Guest SP1 nhận dữ liệu và witness, kiểm tra policy C1–C5, rồi tạo proof Groth16. Contract `EpochAnchor` chỉ ghi epoch/state root khi Solidity verifier chấp nhận proof và epoch chưa từng được anchor.

## 2. Smoke test đã chạy thật

Lần chạy local gần nhất dùng Fabric 2.5 (2 organization, 2 peer, 1 orderer), Fabric Gateway cổng 8081 và Anvil. Tám `EpcisEventV1` canonical, mỗi event 86 byte, được ghi và đọc lại từ ledger trước khi prove.

| Kiểm tra | Kết quả |
|---|---|
| Fabric acknowledgement và ledger readback | 8/8 event thành công |
| SP1 Groth16 prove và local verify | Thành công |
| `verifyAndAnchor` với proof hợp lệ | Accepted; state root được cập nhật |
| Thay một byte proof | Rejected |
| Thay public values | Rejected |
| Anchor lại cùng epoch | Rejected |

Trong lần chạy đó, SP1 proof mất **842.874 giây**, Anvil anchor mất **3.223 giây**, và khoảng từ Fabric acknowledgement đến L1 receipt là **846.097 giây**. Proof fixture dài 356 byte; transaction hợp lệ dùng 293,364 gas trong Anvil. Đây là số đo của **một smoke lot 8 event trên CPU local**, không phải median/P95 của benchmark paper.

## 3. Điều E2 này chứng minh và điều chưa chứng minh

Đã chứng minh ở mức integration: dữ liệu thực đi qua Fabric, proof policy thật được tạo/verify, và EVM không chỉ “anchor chữ ký/proof giả” mà kiểm tra proof trước khi cập nhật state. Đây là khác biệt cốt lõi với E2 lịch sử.

`make e2` và các kết quả lưu trong `results/e2/archive/legacy/` vẫn được giữ để truy vết, nhưng là pipeline mô phỏng: L1 delay mock và proving-time cache. Chúng không được dùng để trả lời RQ2 hiện tại.

Chưa chứng minh:

- Topology triển khai mục tiêu 3 Raft orderer; smoke hiện có 1 orderer để kiểm chứng luồng tối thiểu.
- Workload RQ2: 480 event/phút, 60 phút, 30 seed, lot size ngẫu nhiên 8–64.
- P50/P95/P99 có ý nghĩa thống kê hoặc thông lượng đủ cho production.
- Ethereum public testnet/mainnet; Anvil được chọn để proof verification thật nhưng không phát sinh chi phí mạng thật.

## 4. Kế hoạch để đóng E2

1. Chuyển prover sang hạ tầng GPU phù hợp nếu được cấp quyền; chạy A/B CPU–GPU để xác nhận tốc độ thay vì ước lượng.
2. Chạy smoke lại sau khi nâng topology cần thiết, rồi chạy benchmark RQ2 riêng từng seed, không timing-cache per lot.
3. Đo từ Fabric acknowledgement đến Anvil receipt; tách rõ setup/cache artifact ra khỏi thời gian prove.
4. Xuất raw result ngoài Git, tính median/IQR/P95/P99 và cập nhật paper bằng kết quả đo thật.

**Kết luận:** đường E2E chức năng đã được kiểm chứng thật. E2 với tư cách thí nghiệm latency của paper vẫn đang chờ benchmark quy mô đầy đủ.
