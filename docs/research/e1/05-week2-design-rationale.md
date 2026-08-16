# 05 — Week 2 Design Rationale: Tại sao C2/C5 trước C4 và Recursive Wrapper

> Tài liệu này giải thích các quyết định thiết kế tuần 2 của E1: thứ tự ưu tiên circuit, lý do chọn Poseidon2 thay EdDSA, và thiết kế recursive wrapper.

---

## 1. Mục tiêu tuần 2

Tuần 1 E1 đã thiết lập prototype Plonky3 với circuit C3 (threshold/time baseline). Tuần 2 mở rộng sang:

1. **C2**: Poseidon Merkle membership proof.
2. **C5**: Poseidon nullifier + empty-leaf non-membership MVP.
3. **Recursive wrapper**: Gộp nhiều inner proof (C2 + C3 + C4 + C5) vào một outer proof.

Thứ tự triển khai không phải ngẫu nhiên — có lý do kỹ thuật rõ ràng.

---

## 2. Tại sao C2 và C5 được ưu tiên trước C4?

### 2.1. C4 là gì và tại sao là blocker?

**C4** là component xác minh chữ ký của actor trong chuỗi cung ứng. Phiên bản lý tưởng của C4 sẽ xác minh **EdDSA/Ed25519** — chuẩn chữ ký phổ biến trong blockchain (Solana, Substrate, ...).

**Vấn đề kỹ thuật của EdDSA trong Plonky3**:

EdDSA/Ed25519 hoạt động trên **curve Curve25519** với trường đặc trưng $p_{25519} = 2^{255} - 19$. Trong khi đó, Plonky3 dùng **Goldilocks field** với $p_{GL} = 2^{64} - 2^{32} + 1$.

Để verify EdDSA bên trong STARK Goldilocks, cần **emulate** toàn bộ arithmetic của $\mathbb{F}_{p_{25519}}$ trên $\mathbb{F}_{p_{GL}}$. Điều này kéo theo:

| Chi phí | Ước lượng |
|---------|-----------|
| Số field operation mỗi scalar multiplication | ~hundreds of thousands |
| Số AIR constraint | Cực lớn, có thể > 10M gates |
| Prove time | Không thực tế cho MVP |
| Gadget sẵn có | Không có gadget chính thức trong `p3-goldilocks` pinned rev |

> **Kết luận**: EdDSA/Ed25519 verification bên trong Plonky3/Goldilocks không có gadget đơn giản, không native-friendly, và chi phí prove quá cao cho MVP research. C4 được giữ lại làm **blocker/research item**.

### 2.2. Giải pháp tạm thời: C4 = Poseidon2 Actor Authorization

Để không block toàn bộ benchmark, C4 được thay thế tạm bằng **ZK-native Poseidon2 actor authorization**:

```
commitment = Poseidon2( actor_id, actor_secret, role_tag, 0, 0, 0, 0, 3 )
```

- **Public inputs**: `actor_id`, `role_tag`, `commitment`.
- **Private witness**: `actor_secret`.
- **Ý nghĩa**: Actor chứng minh biết `actor_secret` tương ứng với `commitment` đã đăng ký, mà không tiết lộ secret.

Đây là **pre-image proof** trên Poseidon2, không phải chữ ký. Tuy nhiên đủ để benchmark pipeline và unblock C3 + C5.

### 2.3. Tại sao C2 và C5 khả thi hơn C4?

| Circuit | Hash function | Field | Native? |
|---------|--------------|-------|---------|
| C2 (Merkle membership) | Poseidon2 | Goldilocks | ✅ Yes |
| C5 (Nullifier) | Poseidon2 | Goldilocks | ✅ Yes |
| C4 lý tưởng (EdDSA) | SHA-512 + Curve25519 | ~F_p25519 | ❌ No |
| C4 hiện tại (Poseidon2) | Poseidon2 | Goldilocks | ✅ Yes |

C2 và C5 dùng Poseidon2 trực tiếp trên Goldilocks → constraint degree thấp → prove time hợp lý → **phù hợp với scope MVP tuần 2**.

---

## 3. Thiết kế Recursive Wrapper

### 3.1. Recursive wrapper là gì?

Thay vì gửi 4 proof riêng lẻ (C2, C3, C4, C5) lên on-chain, **recursive wrapper** gộp chúng lại thành **một outer proof duy nhất** xác nhận "tất cả 4 inner proof đều hợp lệ".

**Lợi ích**:
- Giảm on-chain verification cost từ O(n proof) xuống O(1).
- Proof size on-chain nhỏ hơn.
- Phù hợp với mô hình ZK-Rollup.

### 3.2. Cấu trúc 3-layer aggregation

E1 dùng kiến trúc binary aggregation tree:

```
                    [outer_proof]
                   /             \
          [layer_left]        [layer_right]
          /          \        /            \
    [C2_proof]  [C3_proof]  [C4_proof]  [C5_proof]
```

- **Layer 1 trái**: `build_and_prove_aggregation_layer(C2, C3)`.
- **Layer 1 phải**: `build_and_prove_aggregation_layer(C4, C5)`.
- **Layer 2 (final)**: `build_and_prove_aggregation_layer(left_layer, right_layer)`.

Kết quả là một `final_layer` proof xác nhận toàn bộ cây.

### 3.3. API Plonky3 recursion

E1 dùng `p3-recursion` (pinned rev `524665d`) với:

```rust
// Prove inner proof với RecursionConfig
let c2 = prove_recursion_stark(&cfg, &c2_air, c2_trace, &c2_pis)?;

// Wrap thành RecursionInput
let c2_input = RecursionInput::UniStark {
    proof: &c2.proof,
    air: &c2_air,
    public_inputs: c2_pis,
    preprocessed_commit: None,
};

// Aggregate
let left_layer = build_and_prove_aggregation_layer::<RecursionConfig, _, _, _, 2>(
    &c2_input, &c3_input, &cfg, &backend, &params, None,
)?;
```

Tất cả sử dụng **Plonky3 FRI/STARK recursion thực sự** — không có mock hay boolean placeholder.

### 3.4. Trạng thái hiện tại: Blocker

Phiên bản Plonky3 recursion được pin (`524665d`) có **upstream panic** trong aggregation path:

```
thread 'main' panicked at 'trace_next is always present'
```

Nguyên nhân: API nội bộ của `p3-recursion` thay đổi giữa các revision, và rev `524665d` chưa ổn định cho aggregation path đầy đủ.

**Hệ quả trong benchmark**:
- Row `wrapper_recursive_plonky3` có `status = "error"`.
- `note` ghi rõ: `"recursion_blocked: no mocked recursive proof emitted"`.
- **Không** emit mocked proof để tránh overclaim.

Khi upstream ổn định (hoặc khi tìm được rev không panic), wrapper sẽ chạy với `--features recursion`.

---

## 4. Thứ tự triển khai và phụ thuộc

```
Week 1:   C3 (threshold/time) ──────────────────────────────────┐
                                                                 │
Week 2:   C2 (Merkle MVP) ──────────────────────────────────────┤
          C5 (Nullifier MVP) ───────────────────────────────────┤──→ Benchmark grid C2+C3+C4+C5
          C4 (Poseidon2 proxy, không phải EdDSA) ───────────────┘
                                                                 │
          Recursive wrapper (C2+C3+C4+C5) ──────────────────────┴──→ BLOCKED (upstream panic)
          
Future:   C4 EdDSA thực sự ─────────────────────────────────────→ Research item / blocker riêng
          Plonky3-recursion stable rev ──────────────────────────→ Unblock wrapper
```

---

## 5. Phân loại kết quả tuần 2

| Circuit | Trạng thái | Ghi chú |
|---------|-----------|---------|
| C2 Merkle membership | **Implemented + tested** | Poseidon2, depth 16, MVP |
| C3 Threshold/time | **Retained from Week 1** | Tests pass, baseline giữ nguyên |
| C4 Actor authorization | **Implemented (Poseidon2 proxy)** | Không phải EdDSA production |
| C5 Nullifier | **Implemented + tested** | MVP, không phải production accumulator |
| Recursive wrapper | **Blocked** | Upstream panic, không mock |
| C4 EdDSA/Ed25519 | **Research blocker** | Ngoài scope E1 base Plonky3 |

---

## 6. Tại sao không mock recursive proof?

Nguyên tắc thiết kế E1: **không emit mocked proof** dù wrapper bị blocked.

Lý do:
1. **Paper integrity**: Nếu mock → benchmark claim recursion works → overclaim trong paper.
2. **Reproducibility**: Kết quả phải tái lập được trên máy khác với cùng code.
3. **Transparency**: Row error với `note` rõ ràng tốt hơn row "ok" từ mock.

Thay vào đó, benchmark ghi `status = "error"` và `note = "recursion_blocked: ..."` để người đọc hiểu rõ tình trạng.

---

## 7. Tham chiếu code

| Quyết định thiết kế | Vị trí trong code |
|---|---|
| C4 = Poseidon2 thay EdDSA | `templates.rs:build_template()` note cho C4 |
| Wrapper = blocked, không mock | `runner.rs:run_wrapper_row()` nhánh `not(feature="recursion")` |
| Binary aggregation tree | `templates.rs:prove_recursive_wrapper()` |
| C2 public input = root only | `templates.rs:C2MerkleAir::num_public_values()` → 1 |
| C5 public inputs = nullifier + root | `templates.rs:C5NullifierEmptyAir::num_public_values()` → 2 |
