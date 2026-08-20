# Báo cáo E1 - Plonky3 Base Prototype

Ngày cập nhật: 21/08/2026

## 1. Trạng thái hiện tại

E1 hiện dùng Plonky3 core cho code path chính. Base circuits chạy với Goldilocks, Poseidon2, FRI/STARK proof flow.

Các circuit base đang chạy:

| Circuit | Vai trò | Trạng thái |
|---|---|---|
| `c2_poseidon2_merkle_depth16` | Certificate/event membership MVP với Poseidon2 path depth 16 | chạy được |
| `c1_polygon_outside` | Point-in-polygon WGS-84 microdegree với commitment polygon/event | chạy được |
| `c3_threshold_time` | Threshold/time baseline | chạy được |
| `c4_poseidon2_actor_authorization` | Actor authorization bằng Poseidon2 và private actor secret | chạy được |
| `c5_poseidon2_sparse_nullifier_depth32` | Sparse nullifier map 32-bit, bind old/new root, index và state | chạy được |

## 2. Giới hạn cần ghi rõ

- Không claim proof size `196B`; proof size là số đo Plonky3 STARK thực tế.
- C4 trong phạm vi artifact hiện tại là Poseidon2 actor-authorization statement; Ed25519 AIR là hướng mở rộng, không phải claim của phiên bản này.
- C2/C5 là Goldilocks/Poseidon2 circuits; đây chưa phải Solidity-verifiable production accumulator.
- Wrapper đã được migration sang Plonky3-recursion revision `b363397` và có test prove + recursive verify thật cho C1–C5. Solidity/EVM verifier không thuộc phạm vi paper revision hiện tại.
- C4 vẫn là Poseidon2 actor-authorization statement. Fixture EPCIS có chữ ký Ed25519 và host có thể đối chiếu chữ ký, nhưng Ed25519 SHA-512/Curve25519 chưa được ràng buộc trong AIR; không được gọi đây là Ed25519 ZK proof.

## 3. Lệnh kiểm tra

```bash
cargo build
cargo test
cargo run --release -- --events 8,16,32,64 --seeds 3 --circuits c2,c3,c4,c5
```

Kết quả benchmark lịch sử của báo cáo này nằm ở `results/e1/archive/legacy/raw.csv`.

## 4. Kết luận

E1 đã hoàn thiện trong phạm vi paper revision hiện tại: base circuits C1–C5 và recursive wrapper Plonky3 chạy/verify được trong Rust. Paper cần ghi rõ đây là Rust research artifact; Ed25519 AIR, ABI Solidity, Fabric Gateway và Anvil settlement là hướng phát triển tiếp theo, không được trình bày như kết quả đã đo.
