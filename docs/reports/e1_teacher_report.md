# Báo cáo E1 - Plonky3 Base Prototype

Ngày cập nhật: 09/06/2026

## 1. Trạng thái hiện tại

E1 hiện dùng Plonky3 core cho code path chính. Base circuits chạy với Goldilocks, Poseidon2, FRI/STARK proof flow.

Các circuit base đang chạy:

| Circuit | Vai trò | Trạng thái |
|---|---|---|
| `c2_poseidon2_merkle_depth16` | Certificate/event membership MVP với Poseidon2 path depth 16 | chạy được |
| `c3_threshold_time` | Threshold/time baseline | chạy được |
| `c4_poseidon2_actor_authorization` | Actor authorization bằng Poseidon2 và private actor secret | chạy được |
| `c5_poseidon2_nullifier_empty_leaf` | Nullifier + hashed empty-leaf non-membership MVP | chạy được |

## 2. Giới hạn cần ghi rõ

- Không claim proof size `196B`; proof size là số đo Plonky3 STARK thực tế.
- C4 không phải EdDSA/Ed25519 production verification. Ed25519/EdDSA vẫn là blocker riêng.
- C2/C5 là MVP path/hashed empty-leaf model, chưa phải production accumulator.
- Phase 7 đã pin Plonky3-recursion GitHub rev `524665d`, nhưng wrapper recursive vẫn bị chặn vì upstream panic `trace_next is always present` trong aggregation path. Không dùng mocked boolean proof.

## 3. Lệnh kiểm tra

```bash
cargo build
cargo test
cargo run --release -- --events 8,16,32,64 --seeds 3 --circuits c2,c3,c4,c5
```

Kết quả benchmark lịch sử của báo cáo này nằm ở `results/e1/archive/legacy/raw.csv`.

## 4. Kết luận

Base E1 đã đủ để benchmark Plonky3 core cho C2+C3+C4+C5 và unblock hướng E2 nếu chấp nhận recursion là blocker tách riêng. Bước tiếp theo là chờ upstream Plonky3-recursion ổn định hoặc đổi sang rev/API không panic trong aggregation path, sau đó bật lại wrapper recursive Plonky3.
