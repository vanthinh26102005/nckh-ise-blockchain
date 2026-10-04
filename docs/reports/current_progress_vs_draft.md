# Tiến độ hiện tại và các thay đổi so với draft paper

**Ngày cập nhật:** 04/10/2026
**Dành cho:** rà soát hướng nghiên cứu với giảng viên

## Tóm tắt

Nhóm đã có đường kiểm chứng thật **Fabric → ZK proof → Solidity verifier trên Anvil** cho policy C1–C5 từ smoke E2 trước đó. Repo hiện đã bổ sung **E3 recursive epoch guest/anchor, runner so sánh bốn baseline và E4 sweep runner/preflight/analyzer**, cùng topology benchmark ba Raft orderer. E3/E4 đã qua giai đoạn chỉ có ý tưởng, nhưng checkout hiện chưa có gate receipt, CPU/GPU preflight hoặc bộ benchmark chính thức để trả lời RQ3/RQ4 bằng số đo. PR #32 đã cung cấp báo cáo full grid base E1 local CPU (600 phép đo được báo cáo), nhưng raw/source provenance còn chờ reviewer xác thực. E2 full còn thiếu kết quả; chưa gọi toàn bộ paper là hoàn thành.

Bản cập nhật đối chiếu source tới commit `14a7bab` (25/09/2026). Evidence smoke ngày 11/09/2026 được giữ với phạm vi lịch sử; kiểm tra Python/plan mới được chạy ngày 04/10/2026. Báo cáo chi tiết: [E1](e1_teacher_report.md), [E2](e2_teacher_report.md), [E3](e3_teacher_report.md), [E4](e4_teacher_report.md).

Review bổ sung PR #32 ở head `c0e5dfa`: đã nhập report local ngày 29/09 nguyên văn, kiểm tra bảng/lockfile và ghi [giới hạn xác thực E1](../../results/e1/local-cpu-2026-09-29/review.md). PR chưa được merge trong lần cập nhật này; kết quả local không thay thế CPU Slurm issue #25.

## 1. Đối chiếu draft và implementation

| Hạng mục       | Draft định hướng                          | Hiện trạng implementation                                                          | Note                                                                                             |
| ---------------- | --------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| EPCIS            | Dữ liệu truy xuất nguồn gốc              | `EpcisEventV1` canonical 86-byte, dùng xuyên Fabric/prover/test                  | Tránh mỗi layer hash/parse khác nhau                                                          |
| C1–C5           | Policy tuân thủ trong ZK                    | C1 geofence, C2 registry, C3 threshold/time, C4 authorization, C5 nullifier đã có | C4 cần phân biệt baseline Poseidon2 với Ed25519 trong SP1 guest                              |
| E1 / RQ1 | 4 lot size × 30 seed, chi phí mật mã | PR #32 báo cáo 600/600 base C1–C5 trên local CPU; bảng đã nhập | Chờ raw/source evidence và tái tính thống kê; không gồm recursion/SP1/Slurm |
| Recursion        | Plonky3 recursive rollup                      | Plonky3 wrapper Rust cho E1; SP1 epoch guest kiểm chứng và tổng hợp child proofs cho E3 | Hai đường proof khác nhau; gate SP1 vẫn cần receipt của lần chạy thật |
| EVM verification | Draft hướng đến public-chain verification | SP1 Groth16 proof được Solidity verifier kiểm tra và anchor trên Anvil         | EVM verify là thật, nhưng proof system là SP1 deployment path thay vì port Plonky3 verifier |
| Fabric           | Fabric làm private data layer                | Smoke lịch sử có 2 org/2 peer/1 orderer; topology benchmark trong code có 3 Raft orderer | Có cấu hình topology mới chưa đồng nghĩa đã đo benchmark trên topology đó |
| E2 latency       | Ingest đến L1 confirmation                  | Có smoke lịch sử và real benchmark runner/analyzer, pacing theo đồng hồ thực | Chưa có bộ kết quả GPU 60 phút × 30 seed được xác thực |
| E3 / RQ3         | Comparison với các baseline                 | Epoch gate, bốn mode thật, analyzer cho 120 paired run | VeCroToken là bản adapted; chưa có gate manifest và formal comparison trong checkout |
| E4 / RQ4         | Scale study và policy trade-off             | Executor thật, grid 350 run, CPU/GPU preflight, shard merge và analyzer | Plan/test không phải số đo; chưa có sweep GPU chính thức |

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

### 2.5. E3 tổng hợp epoch và so sánh với baseline adapted

SP1 epoch guest kiểm chứng compressed child proofs, bind verification-key digest và nối old/new nullifier root trước khi tạo aggregate Groth16 proof. `EpochAggregateAnchor` anchor một lần mỗi epoch; số proof con tối đa mỗi node là tám, epoch lớn đi qua nhiều tầng recursion. Đây là implementation cho deployment path, không phải Solidity verifier trực tiếp của Plonky3.

E3 đã có Fabric-only, hash-on-chain, VeCroToken-adapted và proposed dưới cùng workload/paired seed. Bản adapted dùng policy/SP1 của nhóm và token anchor tối thiểu, không tái tạo giao thức VeCroToken gốc. So sánh formal cần đủ 4 × 30 run thành công, 60 epoch/run, không censored/saturated và receipt đúng; chưa có dữ liệu đáp ứng điều kiện này trong checkout.

### 2.6. E4 đã có executor thật và grid hai phase

E4 chỉ sweep proposed system. Plan đã lưu gồm 80 screening run và 270 confirmation run; mỗi run bốn epoch, seed deterministic. Runner thật yêu cầu E3 gate và CPU/GPU preflight cùng revision/compute host. Merge/analyzer kiểm tra coverage của plan, giữ run censored/saturated và từ chối mock hoặc split-host khi chạy chế độ chính thức. Dữ liệu synthetic dùng trong test không được đưa vào bảng kết quả paper.

## 3. Mốc implementation có trong repo

| Mốc  | Nội dung                                                                          |
| ----- | ---------------------------------------------------------------------------------- |
| PR#17 | Canonical EPCIS, C1–C5 baseline và Plonky3 recursion Rust                        |
| PR#18 | SP1 policy guest, Fabric chaincode/gateway, Solidity verifier/anchor và E2E smoke |
| PR#19 | Sửa nonce trong Anvil smoke và ghi lại evidence kiểm chứng gần nhất         |
| PR#23 / `22a1641` | Real E2 benchmark runner và topology 3 Raft orderer; merge ngày 22/09/2026 |
| `b5ec031` | E4 plan, harness và phương pháp scalability; 23/09/2026 |
| `58bb113` | E3 recursive SP1 epoch guest, aggregate anchor và code Anvil gate; 25/09/2026 |
| `14a7bab` | Bốn baseline thật, E3 comparison analyzer, E4 executor/preflight/shard merge; 25/09/2026 |
| PR #32 / `c0e5dfa` | Báo cáo full grid base E1 local CPU ngày 29/09; PR open khi review, chưa xác thực raw/source |

Các mốc E3/E4 xác nhận code đã có trong revision đang rà soát; không phải bằng chứng các job GPU hoặc gate đã chạy thành công.

## 4. Trạng thái theo experiment

| Experiment | Implementation               | Evidence hiện có                           | Việc còn lại                                            |
| ---------- | ---------------------------- | -------------------------------------------- | ---------------------------------------------------------- |
| E1 / RQ1   | Có C1–C5 và hai proof paths | Unit/integration/recursion lịch sử; report local 20 ô × 30 seed trong PR #32 | Xác thực raw và source snapshot/diff; tái tạo thống kê/CI/figure; CPU Slurm riêng |
| E2 / RQ2   | Có smoke và real runner/analyzer | Smoke CPU lịch sử 8 event, valid/tamper/duplicate checks | Preflight GPU và 60 phút × 30 seed trên topology 3 Raft; không timing cache |
| E3 / RQ3   | Có recursive gate, 4 mode thật và analyzer | 5 test Python pass; source guest/anchor; chưa có gate receipt/formal results trong checkout | Gate thật rồi 120 paired run, run order, thống kê và giới hạn baseline adapted |
| E4 / RQ4   | Có real executor, preflight, merge và analyzer | 17 test Python pass; plan khớp 80 + 270 run; chưa có sweep measurement trong checkout | Preflight A/B, chạy đủ phase trên cùng compute host, bảng/figure trade-off |

Test Python của E3/E4 kiểm tra logic harness/analyzer và có synthetic/mocked data. Chúng không chứng minh Fabric/SP1/Anvil đã chạy E3/E4 end-to-end. Việc raw được lưu ngoài Git cũng không tự chứng minh đã có số đo: cần cung cấp manifest/receipt và báo cáo được analyzer xác thực.

## 5. Đề xuất bước tiếp theo

1. Xác nhận phạm vi báo cáo hai proof paths và nhãn VeCroToken-adapted trước khi so sánh trong paper.
2. Hoàn tất truy cập HPC H200 và kiểm tra trong GPU allocation: SP1 CUDA, runtime Fabric/Anvil và dung lượng RAM/VRAM. Đã nhận hướng dẫn truy cập không đồng nghĩa preflight benchmark đã pass.
3. Chạy E3 recursive gate trên source revision sạch, lưu proof fixture/receipt ngoài Git và gate manifest đối chiếu được root. Chạy preflight CPU/GPU cho leaf 8/64 event và aggregate hai lot trên topology mục tiêu.
4. Xác thực bundle E1 local trước khi đề xuất chạy lại; CPU Slurm E1 vẫn là deliverable riêng. Chạy E2 full benchmark, E3 120 paired run và E4 80 screening + 270 confirmation run theo protocol. Một topology dùng chung chỉ chạy một run/shard tại một thời điểm; chạy song song phải tách topology và cổng.
5. Với E4 chính thức, Fabric, Gateway, Anvil và prover phải cùng compute host đã preflight. Chạy dịch vụ trên VM và prove trên GPU node là split-host pilot, chưa đáp ứng tiêu chí hiện tại.
6. Xác thực coverage, receipt, censoring và thống kê; sau đó đồng bộ `paper.tex` bằng số liệu thật. Không thay run lỗi/hết thời gian bằng mock.

## 6. Kiểm tra trong lần cập nhật tài liệu

| Kiểm tra ngày 04/10/2026 | Kết quả | Phạm vi |
|---|---|---|
| `python3 scripts/test_e3.py` | 5/5 test pass | Thống kê, paired coverage và synthetic matrix |
| `python3 scripts/test_e4.py` | 17/17 test pass | Plan, wrapper, shard merge và analyzer validation |
| Đối chiếu `results/e4/plan.csv` với `e4_plan.py` | Khớp 350 run; 4 epoch/run | Kiểm tra cấu hình deterministic, không chạy sweep |
| Review PR #32 | Đủ 20 ô; median/IQR hợp lệ; Cargo.lock hash khớp | Không có raw để kiểm tra 600 record hoặc tái tính; source commit không resolve trên remote |

Không chạy lại proof gate, Anvil integration hay benchmark CUDA trong lần sửa docs này. Checkout chưa có `results/e3/gate_manifest.json`, E3 comparison, E4 preflight hoặc phase results để kiểm chứng kết quả chính thức.

**Kết luận:** hướng nghiên cứu vẫn bám bài toán EUDR privacy-preserving traceability của draft, nhưng đã được siết lại để mọi claim deployment/benchmark đều có bằng chứng tương ứng.
