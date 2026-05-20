# Báo Cáo Tuần 1 Gửi Thầy

## Mục tiêu

Nhóm bắt đầu kiểm tra tính khả thi của E1 theo đúng hướng thầy đề xuất: dùng Plonky2 để tạo proof và recursive proof. Trong tuần 1, nhóm chưa triển khai đủ năm sub-circuit C1-C5, mà tập trung dựng một prototype nhỏ nhưng chạy thật để xác nhận pipeline Plonky2 recursion có khả thi.

## Kết quả tuần 1

Nhóm đã tạo project Rust trong thư mục `E1-Ngo_Van_Thinh/`, pin dependency Plonky2 theo tag `v1.1.0`, và dùng nightly toolchain.

Nhóm đã triển khai circuit `threshold-lite`, tương ứng MVP của `C3 threshold`. Circuit kiểm tra mỗi event có reading không vượt ngưỡng và timestamp tăng dần. Dữ liệu reading/timestamp là witness, còn threshold, số event, timestamp đầu và timestamp cuối là public input.

Nhóm đã triển khai recursive wrapper bằng Plonky2. Outer circuit nhận inner proof của `threshold-lite` và verify proof đó bên trong circuit bằng API `verify_proof`. Như vậy prototype đã có recursive proof thật, không phải mock.

Nhóm đã chạy smoke benchmark cho `events/lot = 8, 16, 32, 64` với seed `0, 1, 2`. File kết quả nằm ở `results/week1_smoke.csv`, gồm đủ 12 dòng dữ liệu. Kết quả ban đầu:

- Inner proof size khoảng 89-103 KB.
- Recursive proof size khoảng 127 KB.
- Inner verify time khoảng 2-3 ms.
- Recursive verify time khoảng 3-5 ms.

## Bước tiếp theo

Tuần tiếp theo nhóm nên mở rộng E1 theo thứ tự:

1. Thêm `C2 certificate Merkle membership`.
2. Thêm `C5 nullifier`.
3. Tạo wrapper verify nhiều proof con trong cùng một outer proof.
4. Sau khi C2/C3/C5 ổn định, mới triển khai geofence bản giản lược và đánh giá khả năng làm `C1` đầy đủ.
5. `C4 batched signature verification` nên để sau vì đây là phần cryptographic gadget nặng.

Một điểm cần lưu ý là proof size thực tế của Plonky2 lớn hơn con số 196B trong draft paper. Nhóm sẽ tiếp tục đo bằng artifact thật và đề xuất cập nhật bảng kết quả trong paper theo số liệu thực nghiệm.

