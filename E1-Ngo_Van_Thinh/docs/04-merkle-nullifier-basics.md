# 04 — C2 Merkle Membership và C5 Nullifier: Giải thích kỹ thuật

> Tài liệu này giải thích nguyên lý hoạt động của hai circuit C2 và C5 trong thí nghiệm E1, viết theo ngôn ngữ phù hợp nghiên cứu (research-friendly), có thể trích dẫn trong paper.

---

## 1. Bối cảnh chung

Dự án E1 xây dựng bộ ZK-circuit để chứng minh tính hợp lệ của dữ liệu EPCIS (Electronic Product Chain Information Services) trong chuỗi cung ứng cà phê EUDR. Mỗi lô hàng (lot) được biểu diễn bởi một tập EPCIS event. E1 cần chứng minh:

- **C2**: Certificate hoặc event thuộc về tập hợp hợp lệ (membership).
- **C5**: Lô hàng chưa được xử lý trước đó — tức nullifier chưa tồn tại (non-membership MVP).

Cả hai circuit đều chạy trên **Plonky3** với trường Goldilocks (`p = 2^64 - 2^32 + 1`) và dùng hàm hash **Poseidon2** làm nền tảng mật mã.

---

## 2. C2 — Poseidon Merkle Membership

### 2.1. Vấn đề cần giải quyết

Hệ thống EUDR cần xác minh rằng một certificate (hoặc EPCIS event) **thuộc về tập hợp certificate hợp lệ** mà không tiết lộ giá trị certificate cụ thể. Đây là bài toán _set membership_ trong ZK.

### 2.2. Cấu trúc Merkle Tree

C2 sử dụng **Merkle tree độ sâu cố định 16** (`STRICT_MERKLE_DEPTH = 16`), hỗ trợ tối đa $2^{16} = 65{,}536$ lá (certificates). Mỗi nút được hash bằng **Poseidon2** trên Goldilocks field.

**Định dạng leaf** (lá cây) cho certificate/event:

```
leaf_hash = Poseidon2( cert_id_lo, cert_id_hi, event_value, lot_id, epoch, TAG_CERT, 0, 6 )
                                                                         ↑
                                                               TAG_CERT = 2 (domain separator)
```

Trong đó:
- `cert_id_lo`, `cert_id_hi`: phân rã cert_id thành 2 phần tử trường Goldilocks.
- `event_value`: giá trị event (private witness).
- `TAG_CERT = 2`: domain separator để tránh collision với các circuit khác.
- `6`: số lượng phần tử input có nghĩa (length tag).

### 2.3. Merkle Path Verification

Prover giữ bí mật:
- Giá trị leaf (cert_id, event_value).
- Path sibling hashes từ leaf đến root.
- Index bits (vị trí của leaf trong cây).

Verifier chỉ biết: **Merkle root** (public input).

**Quá trình verify trong circuit** (AIR constraint):

```
level 0: current = Poseidon2(leaf)
level 1: bit=0 → current = Poseidon2(current, sibling_1, TAG_CERT, 0, ..., 3)
         bit=1 → current = Poseidon2(sibling_1, current, TAG_CERT, 0, ..., 3)
...
level 15: current = root
assert current == root_pub  ← public input constraint
```

Tại mỗi level, `bit ∈ {0, 1}` xác định prover đi trái hay phải. Constraint `bit * (bit - 1) = 0` bắt buộc `bit` là boolean.

### 2.4. Public Input

| Tên | Loại | Giá trị |
|-----|------|---------|
| `root` | public | Merkle root của tập certificate hợp lệ |

### 2.5. Tính chất bảo mật

- **Completeness**: Nếu cert thuộc cây và path đúng → proof hợp lệ.
- **Soundness**: Nếu cert không thuộc cây → không thể tạo proof hợp lệ (binding của Poseidon2).
- **Zero-knowledge**: cert_id và giá trị event là private witness, không lộ trong proof.

### 2.6. Giới hạn MVP

C2 hiện là **MVP nghiên cứu**:
- Độ sâu cây cố định 16, không linh hoạt.
- Mỗi proof chỉ chứng minh membership của **một** cert/event.
- Batch membership (nhiều events/lot) cần nhiều C2 proof hoặc thiết kế lại.

---

## 3. C5 — Poseidon Nullifier + Empty-Leaf Non-Membership

### 3.1. Vấn đề cần giải quyết

Để ngăn chặn **double-spending** (cùng một lô hàng được xử lý nhiều lần), hệ thống cần cơ chế chống duplicate. C5 giải quyết bài toán này qua **nullifier pattern**.

### 3.2. Nullifier là gì?

Nullifier là một giá trị dẫn xuất (derived value) từ thông tin riêng tư của lot, được thiết kế sao cho:
1. **Deterministic**: Cùng lot → cùng nullifier.
2. **Hiding**: Không thể suy ngược ra thông tin lot từ nullifier.
3. **Binding**: Mỗi lot chỉ có một nullifier duy nhất.

**Công thức C5**:

```
nullifier = Poseidon2( lot_id, secret, TAG_NULLIFIER, 0, 0, 0, 0, 3 )
                                       ↑
                              TAG_NULLIFIER = 5
```

Trong đó `secret` là giá trị bí mật của prover (private witness). Public output là giá trị `nullifier`.

### 3.3. Empty-Leaf Non-Membership MVP

Để chứng minh nullifier chưa tồn tại (non-membership), C5 dùng mô hình **empty-leaf MVP**:

**Ý tưởng**: Cây Merkle ban đầu có tất cả lá là "empty leaf". Prover chứng minh lá tại vị trí của nullifier vẫn còn empty.

**Empty leaf hash**:

```
empty_leaf_hash = Poseidon2( lot_id, leaf_index, TAG_EMPTY, 0, 0, 0, 0, 3 )
                                                  ↑
                                         TAG_EMPTY = 6
```

**Constraint trong circuit**:
1. Tính `nullifier = Poseidon2(lot_id, secret, TAG_NULLIFIER, ...)`.
2. Tính `empty_leaf = Poseidon2(lot_id, leaf_index, TAG_EMPTY, ...)`.
3. Assert `empty_leaf.lot_id == nullifier.lot_id` (cùng lot).
4. Verify Merkle path từ `empty_leaf` lên `root` — chứng minh vị trí `leaf_index` còn trống.
5. Assert `root == root_pub` (public input).

### 3.4. Public Inputs

| Tên | Loại | Giá trị |
|-----|------|---------|
| `nullifier` | public | `Poseidon2(lot_id, secret, TAG_NULLIFIER, ...)` |
| `root` | public | Merkle root của nullifier set (ban đầu = all-empty) |

### 3.5. Cách hệ thống sử dụng C5

```
1. Prover gửi (proof, nullifier, root) lên chain.
2. Smart contract kiểm tra: nullifier chưa tồn tại trong registry.
3. Nếu hợp lệ → ghi nullifier vào registry.
4. Lần sau: nếu prover gửi cùng nullifier → contract reject.
```

### 3.6. Giới hạn MVP — Không phải Production Accumulator

> **Quan trọng**: C5 là **MVP prototype**, không phải production-grade accumulator.

Hạn chế hiện tại:
- Non-membership chỉ dựa trên hashed empty-leaf — không có cryptographic binding giữa leaf_index và nullifier.
- Không có range proof cho leaf_index nằm trong bounds của cây.
- Cần thiết kế accumulator mạnh hơn (ví dụ: RSA accumulator, Merkle-Patricia trie) cho production.
- Không ngăn được prover chọn leaf_index tùy ý nếu không có ràng buộc thêm ở tầng contract.

---

## 4. So sánh C2 và C5

| Tiêu chí | C2 Merkle Membership | C5 Nullifier Non-Membership |
|---|---|---|
| **Mục đích** | Chứng minh cert/event hợp lệ | Chứng minh chưa dùng lô hàng |
| **Hàm hash** | Poseidon2 (Goldilocks) | Poseidon2 (Goldilocks) |
| **Public inputs** | 1 (Merkle root) | 2 (nullifier + Merkle root) |
| **Private witnesses** | cert_id, event_value, path | lot_id, secret, leaf_index, path |
| **Độ sâu Merkle** | 16 (cố định) | 16 (cố định) |
| **Trạng thái** | Research MVP | Research MVP |
| **Production-ready?** | Chưa (cây tĩnh) | Chưa (cần accumulator mạnh hơn) |

---

## 5. Cách Poseidon2 được dùng trong E1

E1 dùng **Poseidon2** trên **Goldilocks field** với:
- Width = 8 (8 phần tử trường mỗi state).
- Half-full rounds = 4, partial rounds = 22.
- Constants: `GOLDILOCKS_POSEIDON2_RC_8_*` (pinned từ `p3-goldilocks`).

Poseidon2 được chọn thay vì SHA-256 hay Keccak vì:
1. **STARK-native**: Poseidon2 có constraint degree thấp, phù hợp với AIR constraints của Plonky3.
2. **Field-native**: Hoạt động trực tiếp trên Goldilocks field, không cần gadget emulation.
3. **Hiệu quả**: Số lượng constraint nhỏ hơn nhiều so với hash non-native.

---

## 6. Tham chiếu code

| Thành phần | File | Hàm/struct |
|---|---|---|
| C2 AIR constraints | `crates/e1-bench/src/templates.rs` | `C2MerkleAir::eval()` |
| C5 AIR constraints | `crates/e1-bench/src/templates.rs` | `C5NullifierEmptyAir::eval()` |
| C2 witness generation | `crates/e1-bench/src/templates.rs` | `c2_path_witness()` |
| C5 witness generation | `crates/e1-bench/src/templates.rs` | `c5_trace_and_pis()` |
| Poseidon2 permutation | `crates/e1-bench/src/synthetic.rs` | `poseidon2_permute()` |
| Circuit tags/constants | `crates/e1-bench/src/types.rs` | `POSEIDON_TAG_*` |
