# Plonky2 Recursion Basics

## Mục tiêu cần hiểu

Prototype tuần 1 dùng Plonky2 để chứng minh một bước nhỏ của E1: tạo một proof cho circuit `threshold-lite`, sau đó tạo một proof thứ hai để xác minh proof thứ nhất bên trong circuit. Đây là ý nghĩa cốt lõi của recursive proof trong draft paper: verifier không cần kiểm tra từng proof con riêng lẻ, mà có thể kiểm tra một proof đại diện.

## Các khái niệm chính

**Circuit** là tập ràng buộc số học. Trong prototype này, circuit kiểm tra từng event có chỉ số sensor không vượt ngưỡng và timestamp tăng dần.

**Witness** là dữ liệu bí mật đưa vào khi tạo proof. Ở đây witness gồm `readings[]`, `timestamps[]`, `slacks[]`, và `time_deltas[]`.

**Public input** là dữ liệu người verify được thấy. Prototype công khai `threshold`, `event_count`, `first_timestamp`, và `last_timestamp`.

**Proof** là bằng chứng mật mã chứng minh witness thỏa circuit mà không cần đưa toàn bộ witness cho verifier.

**Verify** là bước kiểm tra proof bằng public input và verifying data. Nếu proof hợp lệ, verifier tin rằng có một witness thỏa circuit.

**Recursive proof** là proof chứng minh rằng một proof khác đã được verify đúng bên trong circuit. Trong code, outer circuit dùng `add_virtual_proof_with_pis`, `add_virtual_verifier_data`, và `verify_proof` của Plonky2.

## Inner proof và outer proof

Inner proof:

1. Sinh dữ liệu synthetic cho một lot.
2. Build `threshold-lite` circuit theo số event.
3. Gán witness.
4. Prove và verify circuit.

Outer proof:

1. Nhận inner proof và verifier data.
2. Build circuit mới có logic `verify_proof(inner_proof)`.
3. Prove rằng bước verify inner proof đã đúng.
4. Verify outer proof.

## Vì sao đây là đúng hướng recursive rollup

Trong rollup đầy đủ, mỗi sub-circuit như geofence, certificate, threshold, signature, nullifier sẽ sinh proof riêng. Wrapper circuit sẽ verify các proof đó bên trong một proof lớn hơn. Tuần 1 mới dùng một sub-circuit `threshold-lite`, nhưng luồng kỹ thuật đã đúng: proof con được verify bên trong proof cha bằng Plonky2 recursion.

