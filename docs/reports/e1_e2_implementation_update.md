# E1/E2 Implementation Update

Ngày cập nhật: 11/09/2026
PR: [#17](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/17)
Merge commit: `fa0dfbb`

## 1. Vì sao phải thay đổi nhiều

Paper Stage 3 ban đầu là định hướng nghiên cứu, không phải bản mô tả bắt buộc của artifact. Trong quá trình chạy lại repo, có ba vấn đề cần xử lý:

1. Dependency Plonky3 core và Plonky3-recursion không còn cùng API với revision cũ.
2. Recursive wrapper trước đó bị đánh dấu blocked, trong khi pipeline cần một proof Rust thật để kiểm chứng hướng nghiên cứu.
3. Một số tài liệu cũ mô tả Plonky2, proof `196B`, Sepolia/Fabric và recursive settlement như kết quả đã đo, dù repo chưa có backend tương ứng.

PR #17 xử lý phần Rust proof pipeline và sửa tài liệu theo implementation thực tế. PR không tạo mock proof và không biến host verification thành ZK constraint.

## 2. Thay đổi dependency và cấu hình proving

### Plonky3

- Các crate Plonky3 core được nâng lên version `0.6` từ crates.io.
- `p3-recursion`, `p3-circuit` và `p3-circuit-prover` chuyển sang Plonky3-recursion revision `b36339709a7a67ee9760fb578b3d4339fd983709`.
- Thêm alias `rand_10` cho `rand 0.10.1`, vì constructor `new_from_rng_128` của Plonky3-recursion yêu cầu rand API mới. Workload và fixture cũ vẫn dùng workspace `rand 0.8`.
- `Cargo.lock` được cập nhật để khóa toàn bộ graph dependency mới.

### FRI và Poseidon2

Wrapper dùng một cấu hình cố định, có test kiểm tra trực tiếp:

| Tham số | Giá trị |
|---|---:|
| Goldilocks field | Có |
| Poseidon2 width | 8 |
| FRI log blowup | 2 |
| FRI queries | 8 |
| final polynomial log length | 0 |
| commit/query proof-of-work | 0 |
| max log arity | 1 |

Điểm quan trọng là permutation dùng cho FRI/MMCS/challenger phải giống permutation mà recursion circuit sử dụng. Code hiện tạo `Poseidon2Goldilocks<8>` từ `SmallRng::seed_from_u64(1)`. Trước thay đổi, host dùng Grain constants còn recursion table dùng constants sinh từ seed `1`, dẫn tới lỗi lookup `WitnessChecks`.

Các commitment của EPCIS/C1--C5 vẫn dùng bộ constants của statement tương ứng. Việc đổi permutation recursion không âm thầm thay đổi semantics của event commitment.

## 3. Thay đổi trong E1 circuits

### C1 — geofence

- C1 được đưa vào recursive wrapper, không chỉ chạy như circuit độc lập.
- Polygon dùng tọa độ WGS-84 fixed-point microdegree, không dùng float.
- Host chấp nhận polygon đơn giản từ 3 đến 32 đỉnh và từ chối polygon tự cắt.
- Điểm nằm trên cạnh hoặc đỉnh bị từ chối.
- Test bao phủ điểm trong/ngoài vùng, cạnh/đỉnh, polygon lõm, commitment sai và polygon tự cắt.

### C2 — certificate Merkle membership

- Trace được tổ chức lại thành nhiều row thay vì nhồi toàn bộ path vào một row quá rộng.
- Mỗi row có Poseidon state, index bit, cờ `is_real` và level.
- AIR kiểm tra transition giữa các level, hướng trái/phải của Merkle path và root public input.
- Padding được kiểm soát để recursion verifier nhận đúng `trace_next` columns.

### C3 — threshold/time

C3 giữ nguyên mục tiêu: kiểm tra readings không vượt threshold và timestamp tăng strictly. Trace được đưa vào cùng recursive proving path với các circuit còn lại.

### C4 — actor authorization

C4 hiện là Poseidon2 preimage statement:

```text
Poseidon2(actor_id, actor_secret, role) = commitment
```

Fixture EPCIS vẫn được ký bằng Ed25519 và host có `SignedEpcisEventV1::verify()` để kiểm tra payload, public key và signature. Tuy nhiên, SHA-512, scalar/point canonical encoding và phương trình Curve25519 chưa phải constraints trong AIR. Vì vậy không gọi C4 hiện tại là Ed25519 ZK verification.

### C5 — sparse nullifier

- C5 dùng sparse nullifier map depth 32.
- Public inputs bind `oldRoot`, `newRoot`, nullifier, index và state.
- AIR yêu cầu leaf cũ rỗng khi insert và kiểm tra root update.
- Test bao phủ nullifier trùng, old root sai, new root sai, index/path sai và state sai.

## 4. Recursive wrapper mới

`prove_recursive_wrapper()` hiện thực hiện các bước sau:

1. Tạo trace và public inputs cho C1, C2, C3, C4, C5.
2. Prove từng inner STARK bằng Plonky3.
3. Wrap riêng từng proof C1--C5 qua `build_and_prove_next_layer`.
4. Aggregate C1+C2 và C3+C4.
5. Aggregate (C3+C4)+C5.
6. Aggregate hai nhánh cuối thành final wrapper proof.
7. Gọi `verify_all_tables::<Challenge>()` để kiểm tra recursion tables.

Đây là proof/verification thật trong Rust. Không có nhánh trả về boolean giả hoặc proof mock khi recursion thất bại.

## 5. Thay đổi E2 và tài liệu

E2 hiện sử dụng `EpcisEventV1` canonical và C1 theo lot, nhưng benchmark prototype vẫn có các giới hạn sau:

- C2--C5 timing được cache theo circuit và lot size.
- L1 adapter vẫn là deterministic simulation.
- Chưa có Fabric acknowledgement, Solidity verifier hoặc Anvil receipt trong source tree E2.

Do đó, metadata và report E2 nay gọi đúng đây là **prototype base-proof pipeline latency**, không gọi là Fabric → recursive proof → Anvil end-to-end latency.

Các tài liệu đã cập nhật:

- `README.md`: mô tả dependency, wrapper và giới hạn hiện tại.
- `docs/reports/e1_teacher_report.md`: trạng thái C1--C5 và recursion Rust.
- `docs/reports/e2_teacher_report.md`: phân biệt số liệu lịch sử với phần future work.
- `docs/stages/stage-3/guide.md`: đổi Plonky2/Sepolia claims thành implementation scope và mục tiêu tương lai.
- `docs/stages/stage-3/paper.tex`: loại bỏ claim chưa đo về Plonky2, `196B`, Sepolia, Fabric và settlement latency.
- `docs/research/e1/05-week2-design-rationale.md` và `06-report-to-teacher-week2.md`: đánh dấu các kết luận revision cũ là tài liệu lịch sử.
- `scripts/e1_analyze.py`, `scripts/e2_analyze.py` và E2 metadata: ghi revision và scope đúng với artifact.

Paper hiện tại có thể trình bày E1 như một Rust research artifact hoàn chỉnh trong phạm vi C1--C5 + recursion. Ed25519 AIR, Solidity/EVM và Fabric/Anvil là hướng mở rộng, không phải kết quả đã có.

## 6. Kiểm chứng đã chạy

Các lệnh sau đã pass trên `main` sau khi merge:

```bash
cargo test --workspace
cargo fmt --all -- --check
git diff --check
python3 -m py_compile scripts/e1_analyze.py scripts/e2_analyze.py
```

Kết quả workspace:

- 32 E1 unit tests pass.
- 7 EPCIS integration tests pass.
- 7 E2 tests pass.
- Recursive Poseidon2 parity test pass.
- FRI query configuration test pass.
- Recursive C1--C5 wrapper test pass: 1 test, khoảng 782 giây trên máy chạy.
- `paper.tex` biên dịch thành công bằng Tectonic; các cảnh báo còn lại là font/reference của template IEEE, không phải lỗi syntax.

## 7. Kiểm chứng hạ tầng local sau merge

Ngày 21/08/2026, môi trường local được kiểm tra độc lập với benchmark E2:

- Fabric 2.5.8 chạy bằng Docker với 2 organization, 2 peer và 1 orderer; đây là smoke environment, **không phải** topology 3-Raft orderer của mục tiêu deployment.
- `mychannel` được tạo và cả `peer0.org1` lẫn `peer0.org2` join thành công.
- Chaincode mẫu `basic` được cài, phê duyệt bởi hai organization và commit trên channel. Transaction `CreateAsset` có ID `f9625d46a24dcec8564a5c9632cf51edb6540fd122c06aee19634615de80f64f` nhận trạng thái `VALID` từ cả hai peer; peer Org2 đọc lại đúng asset đã ghi.
- Anvil 1.7.1 chạy ở chain ID `31337`. Một contract probe tối thiểu được deploy qua JSON-RPC, receipt `0x7bfdce8294fce6a74620852092bf3607974e797c11bd904b7e561345f2d51d3a` có `status = 1`.

Đây chỉ chứng minh Docker Fabric, transaction acknowledgement và Anvil RPC hoạt động trên máy. Chaincode đó không phải `e2epcis`; contract probe không nhận hoặc kiểm tra recursive proof. Vì vậy kết quả này **không** là E2 Fabric → recursive proof → Anvil end-to-end và không được đưa vào benchmark/paper như kết quả pipeline.

Khoảng cách kỹ thuật còn lại là cụ thể: `prove_recursive_wrapper()` chỉ trả `RecursiveProofStats`; bytes đang đo bằng `postcard` cho Rust, và repo không có ABI proof hay Solidity verifier cho `p3_batch_stark::BatchProof`. Cần verifier thực thi Goldilocks, Poseidon2, FRI/Merkle và recursive tables trên EVM (hoặc đổi sang proving stack có verifier EVM tương thích) trước khi một receipt Anvil có thể mang ý nghĩa ZK settlement.

## 8. Trạng thái sau merge

- Nhánh làm việc: `main`.
- `main` đồng bộ với `origin/main`.
- PR #17 đã merge.
- Không có generated PDF, raw benchmark CSV hoặc Docker volume mới được commit.

Các hạng mục sau vẫn có thể làm thành PR riêng nếu paper chuyển sang mục tiêu deployment:

1. Ed25519 SHA-512 + Curve25519 AIR cho toàn bộ event trong lot.
2. ABI proof cố định và Solidity verifier chạy trực tiếp trên Anvil.
3. Fabric 2-org/2-peer/3-Raft, Gateway acknowledgement và E2 timing không cache proof.

## 9. SP1/Fabric/Anvil follow-up (04/09/2026)

Nhánh follow-up bổ sung một đường triển khai tách biệt, không thay thế baseline Plonky3:

- `crates/sp1-e2e/`: SP1 guest kiểm tra C1 geofence fixed-point, C2 certificate Merkle path, C3 threshold/time, C4 chữ ký Ed25519 cho từng event, và C5 sparse nullifier update. Journal ABI công khai bind epoch, event/certificate/polygon commitments, old/new nullifier root và kết quả policy.
- `infra/fabric/chaincode/e2epcis/` và Gateway: lưu/đọc canonical EPCIS V1 86-byte, kiểm SHA-256/version/ID trùng, expose `GET /health`, `POST /events`, `GET /events/{id}`.
- `contracts/`: verifier Groth16 SP1 và `EpochAnchor`; contract chỉ cập nhật epoch/root sau khi verifier accept proof, đồng thời chặn epoch trùng. Smoke script kiểm tra proof/public values bị sửa và duplicate epoch bị revert.
- `e2e_smoke`: gửi/read-back 8 event qua Fabric trước khi prove, sau đó mới tạo fixture ABI và gọi smoke Anvil. Không dùng proof cache.

Đã kiểm tra lại nhanh trên nhánh này: 5 policy tests, 3 E2E-runner tests, Go chaincode tests, Go gateway build/tests, Rust format và Solidity compilation đều pass. Local Fabric 2-org/2-peer acknowledgement/read-back cũng đã được chạy trước đó; topology này vẫn là smoke, không phải mục tiêu 3-Raft.

## 10. Kết quả smoke Fabric → SP1 → Anvil (11/09/2026)

SP1 Groth16 circuit v6.1.0 đã được cài hoàn tất, có marker `.complete`; vì vậy lỗi runtime `artifact not found` ở lần kiểm tra trước đã được loại bỏ. Một lượt smoke E2E mới đã chạy từ đầu đến cuối với epoch `1789102714`:

- Fabric 2.5.8 local chạy `e2epcis` với 2 organization/2 peer/1 orderer và endorsement policy của cả Org1 và Org2. Gateway nhận và đọc lại đúng 8 canonical EPCIS V1 events trước khi bắt đầu timing proof.
- SP1 tạo và local-verify một Groth16 proof mới cho đúng lot đó. Thời gian proving sau Fabric acknowledgement là `842,874 ms`; artifact/circuit installation không nằm trong số đo này.
- Solidity SP1 verifier accept proof trên Anvil, `EpochAnchor` cập nhật state root, và receipt hợp lệ là `0x336b2f5c7b911cd2a5355c710f721f479b22e8aac5ccd4e8055f0e43b74ba8d5` (`status = 1`, `293,364` gas).
- Cùng smoke script đã xác nhận proof bị sửa một byte, public values bị sửa một byte và epoch trùng đều bị revert.
- Khoảng từ Fabric acknowledgement tới Anvil receipt là `846,097 ms`, trong đó bước deploy/anchor Anvil là `3,223 ms`. Proof có kích thước `356` bytes.

Kết quả này chứng minh đường **Fabric acknowledgement → C1--C5 SP1 proof → Solidity verification → Anvil receipt** chạy thật, không dùng mock proof hay Rust-only anchor. Fixture proof và dữ liệu đo thô chỉ nằm ở `/tmp`, không được commit.

Smoke này chưa phải benchmark chính và chưa đồng nghĩa đã đạt toàn bộ topology trong paper: môi trường vẫn có 1 orderer thay vì 3 Raft orderer, và chưa có 60 phút × 30 seed độc lập. Hai hạng mục đó vẫn phải chạy riêng rồi mới được đưa số liệu vào paper.
