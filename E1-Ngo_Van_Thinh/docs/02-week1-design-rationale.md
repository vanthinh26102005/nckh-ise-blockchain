# Week 1 Design Rationale

## Vì sao chọn threshold-lite

E1 trong guide yêu cầu đo chi phí mật mã theo số EPCIS event mỗi lot. Trong năm sub-circuit của draft paper, `C3 threshold` là điểm bắt đầu hợp lý nhất vì:

- Dễ ánh xạ từ EPCIS/IoT readings sang arithmetic constraints.
- Không cần Fabric, Sepolia, chữ ký thật, hay dữ liệu địa lý phức tạp.
- Có thể scale trực tiếp theo `events/lot = 8, 16, 32, 64`.
- Đủ để đo prove time, verify time, proof size và recursive proof size.

## Circuit đang chứng minh gì

Với mỗi event, circuit kiểm tra:

- `reading + slack = threshold`
- `slack` nằm trong range hữu hạn, nên reading không vượt threshold.
- `timestamp[i] + delta[i] = timestamp[i+1]`
- `delta[i] >= 1`, nên timestamp tăng nghiêm ngặt.

Các giá trị reading, timestamp, slack và delta là witness. Public input chỉ gồm threshold, số event, timestamp đầu và timestamp cuối.

## Vì sao chưa làm geofence và EdDSA trong tuần 1

`C1 geofence` bản đầy đủ cần point-in-polygon hoặc winding-number trong arithmetic circuit. Phần này nặng vì dữ liệu WGS-84, fixed-point encoding và xử lý cạnh polygon đều dễ làm circuit phình to.

`C4 batched EdDSA` in-circuit cũng nặng vì signature verification trong circuit thường có số constraint lớn. Nếu làm ngay, rủi ro là mất nhiều thời gian vào cryptographic gadget trước khi chứng minh được recursive wrapper.

Tuần 1 vì vậy ưu tiên phần thầy muốn nhất: Plonky2 recursive proof chạy thật. Khi recursion đã ổn, các tuần sau có thể thêm `C2 Merkle membership`, `C5 nullifier`, rồi mới mở rộng `C1/C4`.

## Ghi chú về proof size

Draft paper ghi proof size 196B, nhưng kết quả smoke benchmark với Plonky2/FRI cho thấy proof thực tế lớn hơn nhiều:

- Inner proof: khoảng 89-103 KB trong prototype tuần 1.
- Recursive proof: khoảng 127 KB.

Điều này không làm hỏng hướng nghiên cứu, nhưng cần báo cáo trung thực: 196B phù hợp hơn với các hệ như Groth16, còn Plonky2 dùng FRI nên proof size lớn hơn. Các bảng trong paper nên được cập nhật bằng số đo thực nghiệm sau khi hoàn thiện E1.

