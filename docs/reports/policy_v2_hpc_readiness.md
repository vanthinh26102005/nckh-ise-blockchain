# Báo cáo yêu cầu của thầy và các thay đổi đã thực hiện

Ngày cập nhật: 05/10/2026. Nhánh: `feat/policy-v2-hpc-readiness`.

## Thầy yêu cầu gì?

Bản paper mới và [todo-list của thầy](../../EPCIS_ACIIDS_2026_ThinhTriNhan/todo-list.tex) đặt ra 20 task: sửa ba điều kiện bảo mật B1–B3, chứng minh dữ liệu EPCIS/disclosure, hoàn thiện bằng chứng thực nghiệm E1–E4, quy đổi chi phí và chuẩn bị bản nộp cuối. Các ô đỏ cần số đo hoặc thông tin tác giả phải được giữ cho đến khi có bằng chứng.

Phạm vi đợt này đã chốt: triển khai code và công cụ chạy, kiểm tra nhỏ trên M1/16 GB; đo chính thức và proof thật sẽ chạy trên HPC. C1 plot/multi-polygon và continuity của Aggregation/Transformation giữ future work.

## Đã sửa những gì?

- **B1 — registry enforcement:** một registry chung do owner quản lý; leaf, aggregate và adapted-token anchor chỉ nhận cặp credential root/polygon commitment được duyệt. Guest và aggregate commitment bind hai roots; child kind tường minh; ABI aggregate nâng 288 → 352 byte; server không đăng ký roots từ proof request.
- **B2 — credential/nullifier:** credential bind lot, signer, certificate, role, owner và commitment của nullifier key; index credential tường minh, depth16. HMAC-SHA256 suy ra secret từ key32 byte/lot; sparse map dùng toàn bộ digest256 bit và catalog chặn đổi owner/key giữa snapshot.
- **B3 — hiding commitments:** polygon nonce và batch nonce32 byte; event Merkle leaves có salt riêng, bind index/canonical bytes; batch bind event count/root; disclosure chỉ xuất openings của event được chọn.
- **Mapper/disclosure:** ObjectEvent JSON-LD subset một lot, timestamp có timezone, geo/KPI decimal chính xác, phát hiện ID collision, ký canonical86 byte. CLI mở/verify view kiểm tra commitments, đường aggregation và receipt/log theo chain/anchor/registry/program keys/verifier code được cấu hình độc lập.
- **Harness:** FRI3/33/16, depth6/16 PathB và scope rõ; queue/deadline hữu hạn; manifest workload/registry được chuẩn bị trước; paired modes có ledger riêng; kiểm tra source revision, số event/shipment, receipt/log, coverage/failure và missing metrics. Có preflight, analyzer, Figure2 pipeline, gas/USD Decimal và job dry-run; không tự submit HPC.

## Đối chiếu toàn bộ 20 task

`Đã có code/local test` không đồng nghĩa `đã hoàn tất paper`. Tất cả task vẫn có `formalComplete=false` trong [checklist](../../results/policy-v2/checklist.json).

| Task / yêu cầu | Đã thực hiện | Còn cần |
|---|---|---|
| **T-SEC-01** — Registry binding of RC and cG (condition B1) | Same root pair is bound through typed children and Gamma; registry is owner-controlled; all proof anchors reject unapproved roots. | Final clean source identity, real v2 aggregate Groth16 gate and HPC receipts. |
| **T-SEC-02** — Hiding (salted) commitments (condition B3) | Salted polygon commitment and per-event salted Merkle commitment/openings are implemented. | Final clean source identity, real proof integration and measured disclosure views. |
| **T-SEC-03** — Key-bound nullifier and wider index (condition B2) | Issuer binds lot/owner/key commitment; nullifier is key-derived with a full 256-bit sparse index. | Final clean source identity and real HPC proof/receipt validation; maintain Path B semantic distinction. |
| **T-DM-01** — EPCIS 2.0 JSON-LD to EpcisEventV1 mapper | Strict ObjectEvent subset with canonical 86-byte encoding, collision catalog and explicit signer context. Latest bounded eight-event mapper-to-host-to-guest execution matched the public ABI. | Real proof integration; full JSON-LD and Aggregation/Transformation lot graph remain out of scope. |
| **T-SD-01** — Selective-disclosure view prototype | Three presets and actual CLI fixture tests validate openings, ordered public auth paths and external chain/anchor trust. | Real archived proof tree and receipt, 30 repetitions per view, bundle bytes and measured verify time. |
| **T-C1-01** — Plot-polygon and multi-polygon geofence | Current C1 checks a point against one committed forbidden polygon. | Plot disjointness, all forest-loss polygons and tile completeness; physical lot continuity stays future work. |
| **T-E2-01** — 3-Raft Fabric topology and host record | Allocation-owned topology lifecycle and hardware/software metadata checks exist. | Actual allocation, GPU/driver/CUDA/OS/images/toolchain record and observed 2-org/2-peer/3-orderer preflight. |
| **T-E3-01** — Recursive epoch gate (aggregate proof to Anvil receipt) | Real verifier gate pipeline plus strict protocol/source/root/program/event/tamper metadata checks; contract stubs are excluded. | New v2 compressed leaves, recursive Groth16 proof, successful Anvil receipt, tamper checks and gas/calldata. |
| **T-E1-01** — Path B rerun at production FRI parameters | FRI 3/33/16 and selectable credential depths are represented in source/plans/provenance. | Fresh C1-C5 8/16/32/64 events x 30 seeds at credential depths 6 and 16; no reuse of archived measurements. |
| **T-E1-02** — Path B recursive wrapper benchmark | Separate recursive-wrapper campaign is planned with true circuit labels. | At least n=8/64 x 10 seeds, prove/verify times, postcard proof bytes and peak RSS. |
| **T-E1-03** — SP1 leaf cost, CPU vs GPU A/B | Preflight defines n=8/64, CPU/CUDA, five repetitions and paired workloads on one host. | Measured compressed/Groth16 timings, proof sizes and GPU speedup for final program keys. |
| **T-E2-02** — Full RQ2 latency benchmark | Paced real runner records queue accounting, source, workload/manifest identity and proof receipts. | 480 events/min x60 min x30 seeds; preserve saturation/censoring, tail latency, seed CI and any separately measured sustainable load. |
| **T-E3-02** — RQ3: 4 modes x30 seeds | 120 paired cells with counterbalanced mode order and fresh-ledger isolation; partial reports cannot produce formal p-values. | 120 successful real cells or honest partial status; median/IQR, Wilcoxon/Bonferroni and effect sizes; VeCroToken remains adapted. |
| **T-E4-01** — RQ4 screening (80 runs) | 80-cell screening plan and same-host gate/preflight/manifest validation exist. | Run all80 screening cells; preserve actual offered workload, four completed epochs, saturation/censoring and receipts. |
| **T-E4-02** — RQ4 confirmation (270 runs) and Figure2 | 270 confirmation runs and strict analysis coverage are represented in source. | Real confirmation results and Figure2 median/IQR throughput/completion/gas/calldata; no synthetic paper figure. |
| **T-COST-01** — Gas to fiat conversion | Decimal sensitivity calculator requires explicit dated gas/ETH assumptions and labels estimates. | Insert real measured receipt gas plus contemporaneous cited assumptions; Anvil gas is not a paid mainnet fee. |
| **T-SEP-01** — Optional public-testnet run | Sepolia deployment defaults to no-RPC/no-key dry-run; five-epoch script requires existing real fixtures. | Explicit authorized deployment, five real epochs and measured inclusion delays; otherwise retain Anvil-only limitation. |
| **T-ABS-01** — Headline numbers in the abstract and conclusion | Headline measurement slots remain pending. | Use only accepted E2/E3 raw/metadata/analyzer results; latency/gas ratios remain N/A. |
| **T-DOC-01** — Authors ORCIDs, funding and acknowledgement | Author-provided ORCID/funding information is not supplied by code checks. | Receive and verify each author ORCID, funding/grant and acknowledgement wording. |
| **T-FIN-01** — Final gate before EasyChair | Current readiness checker protects20 IDs and keeps finaltrue disabled. | Formal evidence ledger, author details, complete placeholders, <=15-page/overfull review, explicit final review and submission tag. |

## Kiểm tra đã chạy

- `make local-check`: **54 Rust tests**, **60 Python tests** pass; contract metadata/canonical ABI/Solidity compile pass. **24 test tạo proof được ignore** trong suite mặc định.
- SP1 host `cargo check --all-targets --offline` pass.
- Mapper → ký canonical bytes → private witness → SP1 guest execute **8 event**: public values bằng host; **3,17 giây**, RSS khoảng **74 MiB**, không tạo proof. Cycle counter SDK không có số đo nên ghi `null`, giữ cap50 triệu cycles.
- Contract negative tests đã chạy trên Anvil với verifier stub test-only; container riêng port18545/192 MiB đã dừng. Đây là bằng chứng logic contract, không phải real SP1 proof verification.
- [Evidence local máy đọc được](../../results/policy-v2/local_validation.json) lưu giới hạn và source hashes; dữ liệu private/raw nằm ngoài Git.

## Chưa được coi là hoàn tất

Chưa tạo proof Groth16/recursive v2 mới, chưa có gate/preflight CPU-GPU hoặc receipt proof v2, chưa chạy formal campaign E1–E4, chưa submit Slurm hay broadcast Sepolia. Không thay các số đo cũ bằng fixture và không bật `finaltrue`. ORCID/funding, headline numbers và final submission vẫn chờ.

Paper `.tex` đã cập nhật thiết kế/giới hạn và giữ placeholder đỏ. PDF chưa được nghiệm thu: compiler Tectonic còn tải bộ format lần đầu khi dừng sau khoảng5 phút; máy không có TeX Live.

Review core/consumer đã kiểm tra các sửa đổi bảo mật, queue/watchdog/provenance. Lượt review độc lập toàn branch bị gián đoạn do giới hạn tài khoản; PR cần reviewer kiểm tra trước merge. Nghi ngờ E4 trùng seed giữa cells đã được kiểm tra: canonical350-run plan có350 seed khác nhau, được suy ra từ cell ID và seed index.

Các thay đổi được chia theo boundary test, policy, contracts, host/mapper, disclosure, harness và docs. Compatibility v2 được nghiệm thu trên toàn nhánh sau khi đồng bộ mọi consumer.

Hướng dẫn chuẩn bị config/manifest, thu evidence và chạy HPC: [POLICY_V2_RUNBOOK.md](../POLICY_V2_RUNBOOK.md).
