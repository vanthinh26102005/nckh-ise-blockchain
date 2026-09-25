# E3 — Recursive epoch proof và so sánh bốn hệ thống

**Issue:** [#21](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/21)

**Nhánh bắt đầu:** `feat/e3-recursive-epoch-baselines` (từ `origin/main`, không dùng worktree)

**Mục tiêu:** Chứng minh nhiều leaf policy proof được kiểm tra *bên trong* epoch aggregate proof, anchor proof cha bằng Groth16 thật trên Anvil, rồi so sánh bốn hệ thống trên cùng workload và cùng phương pháp đo.

## Hiện trạng và ranh giới

- SP1 E2 có policy guest, `EpcisEventV1` 86 byte, leaf tối đa 64 event và Groth16 proof được `EpochAnchor.sol` xác thực. E2 real runner đã có Fabric acknowledgement → proof → Anvil receipt.
- E2 hiện dùng một epoch cho một lot. Public values của leaf chưa có `lotId`, `leafIndex` hoặc commitment theo thứ tự của shipment. Nullifier được tính từ `lotId`, nên hai leaf của cùng shipment không thể nối đúng bằng cách chạy lại witness E2. Cần statement leaf riêng/version mới cho E3; giữ nguyên ABI và hành vi của `EpochAnchor.sol`.
- `crates/e2-bench` là artifact lịch sử có proving-time cache và L1 simulation. Không dùng số liệu của nó cho E3; chỉ tái sử dụng generator Poisson/lot khi đã kiểm tra tính phù hợp.
- SP1 SDK đang được pin ở `6.4.0`. Bước đầu phải xác nhận chính xác API deferred/compressed proof của phiên bản này bằng POC có proof con bị sửa và bị guest từ chối. Xác thực proof ở host rồi chỉ truyền boolean vào guest không đáp ứng issue.

## PR 1 — Gate recursion

1. **Đóng băng statement và wire format.** Thiết kế leaf E3 gồm `epochId`, `shipmentId/lotId`, `leafIndex`, `leafCount`, `oldNullifierRoot`, `newNullifierRoot`, `eventBatchDigest`, `eventCount`, `valid` và các policy commitments cần giữ từ E2. Quy định domain separation, kiểu số, ABI encode/decode, vkey và cách tính `leafLotCommitment`. Một shipment 128 event tạo đúng hai leaf 64 event theo chỉ số 0, 1; định danh nullifier của từng leaf phải duy nhất nhưng vẫn gắn với cùng shipment. Viết test codec và test 128 event trước khi đo hiệu năng.
2. **POC proof con thật trong aggregate guest.** Tạo program/host E3 riêng trong `crates/sp1-e2e/` (hoặc package E3 tách riêng nếu build SP1 đòi hỏi), generate leaf proof dạng SP1 hỗ trợ deferred verification, đưa từng proof và public values vào aggregate guest, và verify bằng leaf vkey cố định. Guest kiểm tra fan-in `1..=8`, cùng `epochId`, `valid=true`, `oldRoot[i+1] == newRoot[i]`, thứ tự `shipmentId/leafIndex`, không trùng leaf, tổng `lotCount`/`eventCount` không tràn. `epochCommitment` là fold có domain separation của commitment từng lot/leaf theo thứ tự; không nhận root/commitment do host tự gộp mà không ràng buộc trong proof.
3. **Proof cha và hợp đồng.** Commit đúng bộ public values `(epochId, oldNullifierRoot, newNullifierRoot, epochCommitment, lotCount, eventCount, valid)` từ aggregate guest. Host tạo Groth16 proof cha, local verify bằng aggregate vkey, sau đó deploy `contracts/EpochAggregateAnchor.sol` với verifier SP1 hiện có. Contract kiểm tra vkey/public values, root hiện tại và `valid`, chống anchor lặp `epochId`, cập nhật root và phát event. Giữ `contracts/EpochAnchor.sol` và smoke E2 hoạt động như cũ.
4. **Gate bắt buộc trước benchmark.** Integration test chứng minh 2 × 64 event → 2 leaf proof → proof cha → local verify → Anvil receipt `status=1` và root/commitment đúng. Negative tests: sửa byte proof con; sửa public values; sai vkey; sai epoch; đảo/thay/nhân đôi leaf; đứt nullifier chain; fan-in >8; proof cha sai; `valid=false`; anchor trùng epoch. Test phải quan sát revert hoặc prove/verify failure đúng điểm, không chỉ kiểm tra hàm host trả lỗi.

**Dự kiến tệp:** `crates/sp1-e2e/policy/`, program và host E3 mới dưới `crates/sp1-e2e/`, `contracts/EpochAggregateAnchor.sol`, smoke/negative tests trong `contracts/test/`, manifest statement/vkey trong `docs/`.

**Điểm dừng:** Chưa có POC proof con được xác thực trong guest thì không chạy benchmark chính và không gọi proof cha là recursive epoch proof.

## PR 2 — Baseline comparison (stacked sau PR 1)

1. **Định nghĩa workload chung.** Tạo nhánh `feat/e3-baseline-comparison` từ nhánh PR 1 sau khi gate pass. Tạo runner E3 độc lập và manifest theo seed chứa canonical `EpcisEventV1` bytes, chữ ký, producer ID, shipment/lot mapping, thời điểm Poisson dự kiến và thứ tự event. Cùng manifest được replay cho Fabric-only, Hash-on-chain, VeCroToken-derived adapted và Proposed; mỗi backend có state riêng, cùng cấu hình máy/tài nguyên được ghi lại. Chốt quy tắc deadline, thời gian drain queue, lỗi/timeout và đơn vị shipment trước khi chạy.
2. **Bốn adapter thật.** Fabric-only ghi/đọc lại canonical event trên Fabric. Hash-on-chain ghi digest theo shipment và xác nhận bằng Anvil receipt. VeCroToken-derived adapted mô tả rõ phần token/ownership/kiểm tra được triển khai và khác biệt với bài gốc; dùng transaction/receipt thật cho phần on-chain. Proposed dùng Fabric, leaf proof, epoch aggregate proof và `EpochAggregateAnchor`. Node/ethers nhận, kiểm tra `status`, `gasUsed`, tx hash và calldata bytes từ giao dịch/receipt Anvil. Với Fabric-only, gas và calldata là `N/A` vì không có giao dịch EVM.
3. **Phép đo tương đương.** Mỗi seed chạy cùng lịch Poisson, 125 producer, λ=480 event/phút trong 60 phút; 30 paired seed cho mỗi backend. Pilot `--accelerated` chỉ để debug và được gắn nhãn không phải evidence. Thu raw record cho mọi shipment kể cả fail/timeout: ingest/ready/audit-ready timestamps, trạng thái, proof stages, tx hash, receipt, gas và calldata bytes. Tính completion rate trên toàn bộ manifest, throughput theo shipment/phút và event/phút, audit latency từ thời điểm event cuối được phát theo lịch manifest đến khi bằng chứng có thể truy vấn/kiểm chứng, gas/shipment và calldata bytes/shipment. Ghi riêng thời điểm Fabric acknowledgement để quan sát backlog. Ghi rõ phép đo nào không áp dụng cho từng baseline.
4. **Phân tích và tính tái lập.** Viết `scripts/e3_analyze.py` để kiểm tra đủ bốn baseline × 30 seed, seed pairing, raw schema, receipt hợp lệ và loại pilot; xuất per-seed và aggregate median, IQR, p95, bootstrap 95% CI. Wilcoxon signed-rank trên metric per-seed của các cặp so sánh hợp lệ, hiệu chỉnh Bonferroni cho sáu cặp; nêu cách xử lý tie/missing/failure và có thể báo effect size theo guide. Commit schema, runner, script, metadata (commit SHA, vkey, chain ID, contract addresses, hardware, phiên bản, tham số, command) và methodology; không commit raw data. Không diễn giải adapted VeCroToken là reproduction nguyên bản, không suy ra USD/mainnet từ Anvil.
5. **Kiểm tra và review.** Test manifest replay determinism, adapter readback/receipt, failure accounting và script thống kê bằng fixture nhỏ. Chạy pilot nhỏ rồi gate integration; chỉ sau đó chạy workload chính. Kiểm tra `cargo test --workspace`, test workspace SP1, `cargo fmt --all -- --check`, Solidity/Anvil smoke, Fabric tests và `git diff --check` trước review. PR mô tả riêng kết quả recursion và baseline; tuyệt đối không đưa số liệu pilot vào bảng paper.

**Dự kiến tệp:** runner/package E3 mới trong `crates/` hoặc `crates/sp1-e2e/script/`, adapter Node/ethers trong `contracts/test/`, `scripts/e3_analyze.py`, `results/README.md`, `.gitignore`, README/methodology E3 trong `docs/`.

## Quyết định cần khóa lúc triển khai

- Chọn API và kiểu proof con tương thích SP1 `6.4.0` sau POC, cùng vkey cố định. Nếu cần đổi phiên bản, pin đồng bộ guest/SDK/verifier rồi chạy lại toàn bộ gate.
- Định nghĩa chính xác shipment/lot và commitment để phép đếm `lotCount` không đếm nhầm hai leaf 64 event thành hai shipment.
- Cố định audit-ready predicate và drain deadline cho cả bốn backend trước khi có dữ liệu chính; báo cáo hàng đợi chưa hoàn tất như failure/censored theo methodology, không xóa khỏi mẫu.
