# Tiến độ hiện tại và các thay đổi so với draft paper

**Ngày cập nhật:** 11/09/2026
**Dành cho:** rà soát hướng nghiên cứu với giảng viên

## Tóm tắt một câu

Nhóm đã có đường kiểm chứng thật **Fabric → ZK proof → Solidity verifier trên Anvil** cho policy C1–C5; chưa có bộ benchmark E1/E2 đầy đủ và chưa triển khai E3/E4, vì vậy chưa gọi toàn bộ paper là hoàn thành.

## 1. Đối chiếu draft và implementation

| Hạng mục | Draft định hướng | Hiện trạng implementation | Lý do/thông điệp cần ghi đúng |
|---|---|---|---|
| EPCIS | Dữ liệu truy xuất nguồn gốc | `EpcisEventV1` canonical 86-byte, dùng xuyên Fabric/prover/test | Tránh mỗi layer hash/parse khác nhau |
| C1–C5 | Policy tuân thủ trong ZK | C1 geofence, C2 registry, C3 threshold/time, C4 authorization, C5 nullifier đã có | C4 cần phân biệt baseline Poseidon2 với Ed25519 trong SP1 guest |
| Recursion | Plonky3 recursive rollup | Recursive wrapper Plonky3 prove/verify thật trong Rust | Đây là baseline nghiên cứu, chưa có direct Solidity verifier |
| EVM verification | Draft hướng đến public-chain verification | SP1 Groth16 proof được Solidity verifier kiểm tra và anchor trên Anvil | EVM verify là thật, nhưng proof system là SP1 deployment path thay vì port Plonky3 verifier |
| Fabric | Fabric làm private data layer | 2 org/2 peer/1 orderer smoke, chaincode + gateway thật | Chưa phải topology mục tiêu 3 Raft orderer |
| E2 latency | Ingest đến L1 confirmation | Có một E2E smoke đo Fabric acknowledgement đến Anvil receipt | Chưa phải benchmark 60 phút × 30 seed |
| E3/E4 | Comparison và scale study | Chưa bắt đầu | Không được ghi như kết quả đã có |

## 2. Những thay đổi quan trọng và lý do

### 2.1. Từ “Plonky3 verifier trên Solidity” sang hai đường rõ ràng

Draft hướng tới recursive proof và public verification. Khi implementation, nhóm nhận ra đưa trực tiếp verifier Plonky3 (Goldilocks, Poseidon2, FRI, Merkle và recursion) lên Solidity là một nhánh port lớn, rủi ro cao và làm chậm kiểm chứng câu hỏi chính. Nhóm giữ Plonky3 để nghiên cứu circuit/recursion, nhưng dùng SP1 Groth16 để chạy đường EVM thật.

Đây là thay đổi **phương tiện**, không đổi **mục tiêu**: vẫn chứng minh C1–C5 trên dữ liệu EPCIS private và chỉ anchor khi EVM verifier chấp nhận proof. Paper sau này phải mô tả hai đường này minh bạch, không gộp chúng thành một proof system duy nhất.

### 2.2. Từ mô phỏng latency sang integration có chứng cứ

Số E2 cũ sử dụng L1 mock delay và cache proving time. Nhóm giữ artifact cũ để tái lập lịch sử nhưng loại nó khỏi evidence của RQ2. Smoke hiện tại dùng Fabric transaction acknowledgement, SP1 proof/verify và Anvil receipt thật; đồng thời thử sửa proof/public input/epoch để xác nhận contract từ chối.

### 2.3. Từ public network sang Anvil trước

Anvil cho phép kiểm tra Solidity proof verification thật mà không tốn ETH/testnet quota hoặc phụ thuộc hạ tầng bên ngoài. Vì vậy kết quả hiện tại là local EVM evidence, không phải Sepolia/mainnet performance hay chi phí gas thực tế.

### 2.4. Từ con số dự kiến sang số đo có thể truy vết

Không giữ các claim cũ như proof `196B`, Fabric/Sepolia/recursion “đã chạy”, hoặc benchmark ước lượng. Ví dụ smoke hiện có proof SP1 356 byte; đây không thể so trực tiếp với proof Plonky3 vì khác proving system/cấu hình. Chỉ số nào chưa chạy lặp có kiểm soát đều được đánh dấu pending.

## 3. Mốc implementation đã merge

| Mốc | Nội dung |
|---|---|
| PR #17 | Canonical EPCIS, C1–C5 baseline và Plonky3 recursion Rust |
| PR #18 | SP1 policy guest, Fabric chaincode/gateway, Solidity verifier/anchor và E2E smoke |
| PR #19 | Sửa nonce trong Anvil smoke và ghi lại evidence kiểm chứng gần nhất |

## 4. Trạng thái theo experiment

| Experiment | Implementation | Evidence hiện có | Việc còn lại |
|---|---|---|---|
| E1 / RQ1 | Có C1–C5 và proof paths | Unit/integration/recursion tests | 30-seed benchmark, thống kê và biểu đồ |
| E2 / RQ2 | Có Fabric→SP1→Anvil smoke | 8 event, proof valid/tamper/duplicate checks | 60 phút × 30 seed, không cache proving time |
| E3 / RQ3 | Chưa có baseline runner | Chưa có | Reproduce baseline, cùng workload, statistical comparison |
| E4 / RQ4 | Chưa có sweep runner | Chưa có | Sweep scale/policy và phân tích trade-off |

## 5. Đề xuất bước tiếp theo với thầy

1. Xác nhận có chấp nhận cấu trúc hai đường: Plonky3 cho research baseline, SP1 Groth16 cho EVM deployment validation.
2. Xác nhận budget/hạ tầng GPU cho E1/E2 full run; kết quả GPU phải được đo A/B trên chính workload này.
3. Chốt baseline khả dụng cho E3, đặc biệt VeCroToken, trước khi tuyên bố comparison.
4. Sau khi có dữ liệu E1–E4, sửa `paper.tex` thành paper kết quả; hiện file draft chưa phản ánh implementation mới.

**Kết luận:** hướng nghiên cứu vẫn bám bài toán EUDR privacy-preserving traceability của draft, nhưng đã được siết lại để mọi claim deployment/benchmark đều có bằng chứng tương ứng.
