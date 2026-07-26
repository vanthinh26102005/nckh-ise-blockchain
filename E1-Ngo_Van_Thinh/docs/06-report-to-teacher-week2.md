# Báo cáo Tuần 2 — E1 Cryptographic Benchmark

**Người viết**: Nhân
**Ngày**: 07/06/2026
**Phạm vi**: 01/06/2026 – 07/06/2026

> Báo cáo này dựa trên kết quả implementation của Trí — branch `chore/update-results-config`. Số liệu benchmark chạy thực tế trên máy Trí lúc `2026-06-07T02:29:03 UTC` bằng lệnh `make e1-strict`.

---

## 1. Mục tiêu tuần 2

Tuần 2 E1 mở rộng prototype Plonky3 từ tuần 1 theo 3 hướng:

1. **Thêm C2**: Poseidon Merkle membership proof — chứng minh certificate/EPCIS event thuộc tập hợp hợp lệ.
2. **Thêm C5**: Poseidon nullifier + empty-leaf non-membership MVP — chứng minh lô hàng chưa được xử lý trước đó (chống double-spending).
3. **Recursive wrapper**: Thử nghiệm gộp C2 + C3 + C4 + C5 thành một outer proof duy nhất bằng Plonky3-recursion.

---

## 2. Kết quả đã thực hiện

### 2.1. Trạng thái các circuit

| Circuit | Tên đầy đủ | Trạng thái | Ghi chú |
|---------|-----------|-----------|---------|
| C2 | `c2_poseidon2_merkle_depth16` | ✅ Chạy được | Poseidon2 Merkle depth 16, MVP |
| C3 | `c3_threshold_time` | ✅ Giữ nguyên từ tuần 1 | Baseline, tests pass |
| C4 | `c4_poseidon2_actor_authorization` | ✅ Chạy được | Poseidon2 proxy; **không phải** EdDSA |
| C5 | `c5_poseidon2_nullifier_empty_leaf` | ✅ Chạy được | Nullifier MVP, hashed empty-leaf |
| Wrapper | `wrapper_recursive_plonky3` | ❌ Blocked | Upstream panic, **không mock** |

### 2.2. Môi trường benchmark

| Thông tin | Giá trị |
|-----------|---------|
| **Máy chạy** | Apple Silicon Mac (arm64, macOS 26.5) |
| **CPU logical** | 8 cores |
| **Rust** | `rustc 1.97.0-nightly (e50aa6fba 2026-05-19)` |
| **Cargo** | `1.97.0-nightly (4d1f98451 2026-05-15)` |
| **Backend** | Plonky3 core |
| **Hash** | Poseidon2 (Goldilocks, width 8) |
| **Plonky3 core rev** | `56952503e1` |
| **Plonky3-recursion rev** | `524665d0c2` (pinned, research prototype) |
| **Lệnh** | `make e1-strict` |
| **Config** | `events/lot ∈ {8,16,32,64}`, 30 seeds/cell, profile `coffee-default` |
| **Tổng rows** | 480 (4 circuits × 4 event sizes × 30 seeds) |

### 2.3. Kết quả benchmark — Mean (95% CI bootstrap, 30 seeds/cell)

#### C2 — `c2_poseidon2_merkle_depth16`

| events/lot | Prove (s) | Verify (ms) | Proof size (bytes) | Peak RAM (GB) |
|-----------|----------:|------------:|-------------------:|-------------:|
| 8  | 0.01041 [0.01031–0.01053] | 11.454 [10.268–13.744] | 275,502 | 0.00889 |
| 16 | 0.01051 [0.01038–0.01064] | 10.475 [10.373–10.583] | 275,635 | 0.00956 |
| 32 | 0.01025 [0.01017–0.01035] | 10.298 [10.198–10.414] | 275,680 | 0.00970 |
| 64 | 0.01291 [0.01071–0.01695] | 11.205 [10.406–12.722] | 275,652 | 0.00979 |

#### C3 — `c3_threshold_time`

| events/lot | Prove (s) | Verify (ms) | Proof size (bytes) | Peak RAM (GB) |
|-----------|----------:|------------:|-------------------:|-------------:|
| 8  | 0.00113 [0.00112–0.00114] | 0.533 [0.527–0.542] | 11,090 | 0.00890 |
| 16 | 0.00208 [0.00206–0.00210] | 0.608 [0.599–0.619] | 13,425 | 0.00956 |
| 32 | 0.00396 [0.00393–0.00400] | 0.672 [0.668–0.677] | 16,082 | 0.00971 |
| 64 | 0.00792 [0.00783–0.00803] | 0.773 [0.764–0.784] | 19,039 | 0.00979 |

#### C4 — `c4_poseidon2_actor_authorization`

| events/lot | Prove (s) | Verify (ms) | Proof size (bytes) | Peak RAM (GB) |
|-----------|----------:|------------:|-------------------:|-------------:|
| 8  | 0.000397 [0.000393–0.000402] | 0.436 [0.423–0.459] | 11,695 | 0.00891 |
| 16 | 0.000398 [0.000391–0.000406] | 0.435 [0.426–0.446] | 11,695 | 0.00956 |
| 32 | 0.000401 [0.000391–0.000413] | 0.424 [0.421–0.429] | 11,696 | 0.00971 |
| 64 | 0.000413 [0.000399–0.000430] | 0.437 [0.428–0.447] | 11,704 | 0.00979 |

#### C5 — `c5_poseidon2_nullifier_empty_leaf`

| events/lot | Prove (s) | Verify (ms) | Proof size (bytes) | Peak RAM (GB) |
|-----------|----------:|------------:|-------------------:|-------------:|
| 8  | 0.01043 [0.01032–0.01056] | 10.378 [10.292–10.469] | 276,173 | 0.00896 |
| 16 | 0.01055 [0.01042–0.01069] | 10.478 [10.376–10.584] | 276,200 | 0.00956 |
| 32 | 0.01055 [0.01022–0.01109] | 10.344 [10.174–10.617] | 276,238 | 0.00971 |
| 64 | 0.01061 [0.01048–0.01075] | 11.223 [10.413–12.747] | 276,189 | 0.00979 |

### 2.4. Nhận xét về proof size

- Proof size thực tế đo được: **~11–276 KB** (postcard serialization của Plonky3 STARK proof).
- C2 và C5 lớn hơn do phải encode toàn bộ Poseidon2 AIR trace cho Merkle path depth 16.
- C3 và C4 nhỏ hơn đáng kể (~11–19 KB) do trace đơn giản hơn.
- **Không claim** `196B` — đây là target lý thuyết trên paper, chưa đạt được với Plonky3 FRI/STARK ở giai đoạn này.

---

## 3. Giới hạn hiện tại

### 3.1. C4 — EdDSA/Ed25519 chưa production-ready

E1 dùng Plonky3 làm backend proving và recursive aggregation. C4 là component xác minh chữ ký. EdDSA/Ed25519 verification khó implement hiệu quả bên trong Plonky3 vì Ed25519 không native-friendly với Goldilocks field và không có gadget chính thức đơn giản trong pinned dependency. Do đó, Tuần 2 coi C4 là **blocker/research item** và **không claim production EdDSA verification**.

C4 hiện chạy bằng **Poseidon2 actor authorization proof** (pre-image proof thay thế tạm thời):
`commitment = Poseidon2(actor_id, actor_secret, role_tag, 0, 0, 0, 0, 3)`

### 3.2. Recursive wrapper — Blocked

Wrapper dùng `p3-recursion` (rev `524665d`) bị **upstream panic** trong aggregation path (`trace_next is always present`). Row `wrapper_recursive_plonky3` ghi `status = "error"` và không phát ra mocked proof để đảm bảo tính trung thực của benchmark.

### 3.3. C2 và C5 — MVP, chưa production

- **C2**: Độ sâu cây cố định 16, mỗi proof chứng minh 1 certificate, cây tĩnh.
- **C5**: Non-membership dựa trên hashed empty-leaf — mô hình MVP đơn giản, chưa phải production accumulator.

---

## 4. Phân loại: Implemented / MVP / Blocker

| Hạng mục | Phân loại | Chi tiết |
|----------|----------|---------|
| C2 Merkle membership | **Implemented + tested** | Chạy, có test valid + invalid |
| C3 Threshold/time | **Implemented (tuần 1, giữ nguyên)** | Tests pass |
| C4 Poseidon2 auth | **Implemented (proxy tạm)** | Không phải EdDSA thực |
| C5 Nullifier | **MVP prototype** | Hashed empty-leaf, non-production |
| Recursive wrapper | **Blocked** | Upstream panic, no mock |
| C4 EdDSA/Ed25519 | **Research blocker** | Ngoài scope E1 base Plonky3 |
| Proof size 196B | **Chưa đạt** | Target paper, không claim |

---

## 5. Bước tiếp theo

1. **Unblock Wrapper**: Theo dõi upstream `Plonky3-recursion`; thử rev mới khi có bản ổn định.
2. **Cải thiện C4**: Nghiên cứu chiến lược EdDSA khả thi (BabyJubJub, Ristretto, hoặc field-native scheme).
3. **Mở rộng benchmark**: Tăng seeds, mở rộng event grid nếu runtime cho phép.
4. **Căn chỉnh paper claims**: Đảm bảo mọi số liệu trong draft paper tham chiếu `results/e1/raw.csv`, không dùng estimate.

---

## 6. Tài liệu tham chiếu

| Tài liệu | Đường dẫn |
|---------|-----------|
| Giải thích kỹ thuật C2 + C5 | `E1-Ngo_Van_Thinh/docs/04-merkle-nullifier-basics.md` |
| Lý do thiết kế tuần 2 | `E1-Ngo_Van_Thinh/docs/05-week2-design-rationale.md` |
| Báo cáo tuần 1 | `reports/e1_teacher_report.md` |
| Raw benchmark CSV | `results/e1_raw.csv` (480 rows) |
| Summary table CSV | `results/e1_table1.csv` |
| Auto-generated report | `results/e1_report.md` |
| Benchmark metadata | `results/e1_metadata.json` |
