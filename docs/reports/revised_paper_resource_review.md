# Review bản paper và tài nguyên thầy gửi ngày 04/10/2026

**Nguồn:** [EPCIS_ACIIDS_2026_ThinhTriNhan](../../EPCIS_ACIIDS_2026_ThinhTriNhan/).
**Phạm vi:** đọc source paper, bibliography, checklist và hai PDF; đối chiếu với code/results ở nhánh `docs/revised-paper-resource-review`, bắt đầu từ commit `861f34e`. Không chạy thêm benchmark hoặc sửa bản paper gốc trong lần review này.

## 1. Thầy đã đưa những gì?

| Tài nguyên | Nội dung và cách dùng |
|---|---|
| [main.tex](../../EPCIS_ACIIDS_2026_ThinhTriNhan/main.tex) | Bản thảo mới theo LNCS, tên **Privacy-Preserving, Publicly Verifiable EUDR Compliance over EPCIS Event Data with Recursive Zero-Knowledge Proofs**. Có mô hình dữ liệu, quan hệ C1–C5/epoch, phân tích bảo mật, bảng thực nghiệm và các vị trí cần hoàn thiện. |
| [todo-list.pdf](../../EPCIS_ACIIDS_2026_ThinhTriNhan/todo-list.pdf), [todo-list.tex](../../EPCIS_ACIIDS_2026_ThinhTriNhan/todo-list.tex) | Checklist 5 trang, **20 task**: yêu cầu sửa code, lệnh chạy, tiêu chí chấp nhận, đầu ra, phần paper được điền, thời gian ước tính và người phụ trách được đề xuất. |
| [references.bib](../../EPCIS_ACIIDS_2026_ThinhTriNhan/references.bib) | **29 mục tham khảo**: EUDR và sửa đổi, EPCIS/CBV, traceability, ZK supply chain, geolocation, selective disclosure, verifiable SQL, FRI/Groth16/Poseidon2/Ed25519, SP1 và Plonky3. Đây là metadata/DOI/URL, không phải 29 bài PDF tải sẵn. |
| [architecture.pdf](../../EPCIS_ACIIDS_2026_ThinhTriNhan/figures/architecture.pdf), [architecture.tex](../../EPCIS_ACIIDS_2026_ThinhTriNhan/figures/architecture.tex) | Sơ đồ ba vùng Fabric L0 → prover off-chain → EVM L1, phân biệt Path A/SP1 và Path B/Plonky3, registry và đường gửi openings cho auditor. Source TikZ có thể chỉnh sửa. |
| [draw-paper.sty](../../EPCIS_ACIIDS_2026_ThinhTriNhan/figures/draw-paper.sty) | Bộ style TikZ/PGFPlots cho sơ đồ và biểu đồ thống nhất màu, đường nối, font và kích thước. |
| [llncs.cls](../../EPCIS_ACIIDS_2026_ThinhTriNhan/llncs.cls), [splncs04.bst](../../EPCIS_ACIIDS_2026_ThinhTriNhan/splncs04.bst) | Class/bibliography style của LNCS để dàn trang paper và tài liệu tham khảo. |

Folder này là **bộ manuscript + hướng dẫn hoàn thiện nghiên cứu**. Không có dataset forest-loss đã xử lý, bộ EPCIS thực tế, raw benchmark mới, tài khoản HPC hoặc cấu hình Slurm mới. Hansen/GFW là nguồn dữ liệu được đề xuất để xây polygon; GS1 là nguồn ví dụ để kiểm tra mapper. Các nguồn đó vẫn cần lấy và xử lý khi triển khai task tương ứng.

Tên folder ghi `2026`, nhưng source và todo-list ghi **ACIIDS 2027**. Checklist ghi hạn nộp **31/10/2026**, với mục tiêu submit ngày 30/10. Đây là mốc trong tài liệu thầy gửi; lần review này chưa xác minh lịch hội nghị bên ngoài. Folder chưa có PDF tổng của `main.tex`.

## 2. Paper đã cập nhật gì so với bản Stage 3?

Bản [Stage 3 cũ](../stages/stage-3/paper.tex) dùng IEEEtran và còn mô tả ranh giới artifact trước khi có Fabric/SP1/Anvil. Bản mới đã đưa các tiến bộ implementation vào paper:

1. **Định vị rõ hệ thống ZK-EUDR và hai proof paths.** Path A dùng SP1 kiểm tra policy với Ed25519 thật, recursive fan-in tối đa tám, rồi wrap root thành Groth16 để EVM kiểm tra. Path B dùng Plonky3 AIR C1–C5 và recursive wrapper để nghiên cứu chi phí; C4 là Poseidon2 authorization preimage. Không dùng Path B để settlement.
2. **Định nghĩa chính xác hơn điều cần chứng minh.** Có canonical event 86 byte, quan hệ tuân thủ từng lot, quan hệ epoch nối nullifier roots, số lot/event và verification-key digest binding.
3. **Bổ sung phân tích bảo mật có điều kiện.** Proposition về settlement soundness chỉ có giá trị dưới các điều kiện B1–B3. Paper chỉ rõ code hiện chưa thực thi đầy đủ các điều kiện này; proof verify thành công chưa tự chứng minh các registry/secret do prover chọn là hợp lệ.
4. **Đưa evidence thật vào phần functional validation.** Smoke E2 một lot tám event: proof 356 byte, 293,364 gas, khoảng 846.1 giây từ Fabric acknowledgement đến Anvil receipt; tampered proof/public values và replay đều bị từ chối. Paper ghi đây là một lần CPU smoke, topology một orderer, trước các sửa B1–B3.
5. **Siết phương pháp E1–E4.** Tách smoke khỏi formal benchmark, paired seeds, IQR/percentiles/CI, Wilcoxon/Bonferroni; chỉ điền số có provenance. E3 có bốn mode và E4 có 80 screening + 270 confirmation run.
6. **Mở rộng related work và giới hạn claim.** Có bảng so sánh PrivChain, Hybrid EPCIS, zkFabLedger, zkLP, Ocean DPP, VeCroToken và OreProof. Theo bản thảo, Ocean DPP là đối chiếu gần với EPCIS + ZKP; VeCroToken-adapted là ablation dùng policy của nhóm, không tái tạo giao thức VeCroToken gốc. Các mô tả related work này là nội dung bản thảo, chưa được lần review này xác minh bằng cách đọc từng bài bên ngoài.
7. **Nêu giới hạn thực tế cụ thể.** Chưa có mapper JSON-LD hoàn chỉnh, selective disclosure còn là prototype cần làm, C1 hiện kiểm tra điểm với một polygon, Anvil không đo độ trễ mạng public chain, và proof không đảm bảo GPS/sensor đúng ngoài đời.

## 3. Những chỗ màu đỏ có nghĩa gì?

Trong `main.tex`, `\pending{ID}{nội dung}` và `\pendcell{ID}` tạo phần màu đỏ. Có **33 vị trí đỏ, tương ứng 19 ID duy nhất**. Todo-list có thêm `T-FIN-01`, tổng cộng 20 task. Một ID có thể xuất hiện nhiều lần, ví dụ toàn bộ bảng E3 dùng chung `T-E3-02`.

Màu đỏ gồm cả **sửa implementation**, **chạy thực nghiệm**, **hoàn thiện chức năng** và **thông tin tác giả**. Vì vậy không thể chỉ lấy các con số đang có rồi điền hết. Khi bật `\finaltrue`, hai macro báo lỗi nếu còn placeholder. Màu xanh của `\rev{...}` chỉ hiện nếu bật `MARKREV`; khác với task pending màu đỏ.

### 3.1. Ba sửa bảo mật cần làm trước benchmark chính thức

| ID | Vấn đề thầy chỉ ra | Đối chiếu code và yêu cầu hoàn thiện |
|---|---|---|
| **T-SEC-01 / B1** | Prover có thể tự chọn credential root/polygon và chứng minh trên chúng. | [epoch.rs](../../crates/sp1-e2e/policy/src/epoch.rs) chưa truyền hai root này ra epoch statement, [EpochAggregateAnchor.sol](../../contracts/EpochAggregateAnchor.sol) chưa kiểm tra registry. Cần cùng root ở mọi child, bind vào epoch commitment, ABI epoch **288 → 352 byte**, và registry kiểm tra ở cả aggregate anchor lẫn [leaf anchor](../../contracts/EpochAnchor.sol). Test mismatch/unregistered root phải fail. |
| **T-SEC-03 / B2** | Một lot có thể dùng hai secret khác nhau để tạo hai nullifier khác nhau. Chỉ số 32 bit dễ va chạm khi quy mô tăng. | [PolicyInput/evaluate](../../crates/sp1-e2e/policy/src/lib.rs) nhận `nullifier_secret` từ witness; credential leaf chưa bind nullifier key, và `nullifier_index` dùng bốn byte. Cần secret suy ra từ key đã bind vào credential, guest kiểm chứng, và index rộng 256 bit hoặc thiết kế indexed tree. Test cùng lot với hai secret phải bị từ chối. |
| **T-SEC-02 / B3** | Hash không salt của polygon/events có thể bị dò từ tập dữ liệu ít khả năng. | `polygon_commitment` và event batch digest hiện không có nonce. Cần salted commitments và Merkle root cho events để vừa tăng tính hiding vừa hỗ trợ mở một phần dữ liệu. |

Các nhận xét trên khớp source ở revision được review; đây là đối chiếu tĩnh, chưa thực hiện khai thác hoặc sửa lỗi. Sau khi sửa cần cập nhật witness builders, fixtures, Rust/Solidity ABI, guest/program keys, adapter và negative tests cùng nhau. Với B1, fixture phải dùng registry root thống nhất thay vì để mỗi lot tự dựng một credential tree riêng.

**Hệ quả cho số liệu:** giữ kết quả cũ dưới nhãn cấu hình lịch sử. Sửa guest/ABI/hash/tree làm thay đổi program keys, proof và chi phí; cần gate/preflight và benchmark của revision mới trước khi dùng số liệu để khẳng định hệ thống đã sửa xong.

### 3.2. Chức năng và fidelity của dữ liệu

| ID | Việc phải làm | Mức ưu tiên theo checklist |
|---|---|---|
| **T-DM-01** | Mapper EPCIS 2.0 JSON-LD → `EpcisEventV1`: eventID, LGTIN, geo URI, time, sensor, certification và role; kiểm tra ví dụ GS1, từ chối thiếu geolocation. | Mapper là phần cần hoàn thiện; lot graph của Aggregation/Transformation là stretch goal, nếu trễ giữ future work. |
| **T-SD-01** | CLI mở/kiểm chứng compliance view cho regulator/importer/certifier; đo bundle bytes và verify time, 30 lần/view. | Phụ thuộc T-SEC-02; hiện chưa có evidence prototype trong checkout. |
| **T-C1-01** | Từ kiểm tra điểm ngoài một polygon sang plot-polygon disjointness và kiểm tra mọi forest-loss polygon trong tile, kèm chứng cứ danh sách tile đầy đủ. | Checklist cho phép giữ limitation/future work nếu chưa kịp; không ghi “đã đáp ứng đầy đủ” khi chỉ kiểm tra điểm. |
| **T-SEP-01** | Anchor năm epoch trên Sepolia và đo inclusion delay. | Tùy chọn; nếu không làm, xóa slot và giữ rõ “Anvil only”. |

### 3.3. Thực nghiệm E1–E4 và các số cần điền

| ID | Cần đo/điền | Hiện trạng so với yêu cầu mới |
|---|---|---|
| **T-E2-01** | Fabric 2 org / 2 peer / **3 Raft orderer** và thông số CPU/RAM/GPU/driver/CUDA/OS/images/toolchain. | Topology đã có trong code; chưa có preflight report chính thức trong checkout. |
| **T-E3-01** | Gate hai lot × tám event → compressed leaves → recursive Groth16 → Anvil receipt, root đúng, tamper rejected; đo aggregate gas/calldata. | Gate code đã có, nhưng checkout chưa có `results/e3/gate_manifest.json`. Không lấy E2 single-leaf smoke thay aggregate gate. |
| **T-E1-01** | Rerun Path B C1–C5, bốn lot size × 30 seed với FRI mới; so sánh credential depth 6/16 và mô tả khác biệt hash/C4. | Source hiện `log_blowup=2`, tám queries, grinding=0. Checklist đề xuất 3/33/16 và ước tính khoảng 115 conjectured bits; đây là ước tính theo mô hình trong tài liệu, không phải security audit. PR #32 báo cáo 600 phép đo local nhưng raw/source còn chờ xác thực và chưa chứng minh đã dùng cấu hình mới. |
| **T-E1-02** | Benchmark Plonky3 recursive wrapper ở n=8/64, 10 seed: prove/verify/proof bytes/peak RSS. | Wrapper đã có; smoke chạy được không thay benchmark lặp này. |
| **T-E1-03** | SP1 CPU/GPU A/B: leaf n=8/64 và aggregate hai lot, tối thiểu năm lần/cấu hình; tách compressed proof và Groth16 wrap. | Preflight code là điểm khởi đầu; chưa có bộ đo đáp ứng số lần lặp và toàn bộ timing này. |
| **T-E2-02** | Real runner: 480 events/phút, 60 phút × 30 seed; median/IQR/P95/P99, saturation và offered load bền vững nếu cần. | Chỉ có smoke lịch sử; không dùng pipeline `e2-bench`/timing cache để điền RQ2. |
| **T-E3-02** | Bốn mode × 30 paired seed = **120 run**; throughput/gas/calldata/latency và thống kê so sánh. | Runner/analyzer đã có; chưa có `results/e3/comparison.json`. Formal analysis yêu cầu đủ bộ thành công, không suy từ partial smoke. |
| **T-COST-01** | Script quy đổi gas → fiat với gas price, ETH/USD, ngày/nguồn và sensitivity range. | Đây là phép tính theo giả định giá; không coi gas đo trên Anvil là phí mainnet đã trả. |
| **T-E4-01** | **80 screening run** cho proposed system. | Plan có; chưa có sweep measurement. |
| **T-E4-02** | **270 confirmation run** và Figure 2 throughput/completion/gas/calldata, median/IQR và đánh dấu censored/saturated. | Plan có; figure/results chính thức chưa có. Confirmation hiện được định trước trong `e4_plan.py`. |

Table RQ1 trong paper dùng **archive C2–C5**: C5 còn ghi “empty leaf”, proof khoảng 276 kB. Report local mới trong [PR #32](https://github.com/vanthinh26102005/nckh-ise-blockchain/pull/32) có C1 và C5 update khoảng 2.17 MB, dùng median/IQR, không cùng cấu hình/phiên bản với bảng cũ dùng mean. Không trộn hai bảng hoặc điền các hàng production/recursion/SP1 bằng số của report base local. [Review evidence E1](../../results/e1/local-cpu-2026-09-29/review.md) ghi rõ phần còn thiếu.

### 3.4. Hoàn thiện manuscript

| ID | Đầu ra |
|---|---|
| **T-ABS-01** | Chỉ sau E2/E3, điền headline latency và mức giảm gas có số đo vào abstract/conclusion. |
| **T-DOC-01** | ORCID từng tác giả, funding/grant và acknowledgement. |
| **T-FIN-01** | Compile với `\finaltrue`, kiểm tra ≤15 trang/overfull boxes, evidence ledger, review và tag release paper. |

## 4. Điểm cần đồng bộ trước khi dùng checklist để chạy

1. **Đường dẫn manuscript:** todo dùng `aciids2027/main.tex` và `aciids2027/evidence_ledger.md`, nhưng repo hiện dùng folder thầy vừa thêm và chưa có ledger. Cần chọn đường dẫn chính thức và tạo bảng truy vết mỗi số đo → revision/metadata/raw artifact trước khi điền paper.
2. **GPU:** bản thảo/checklist ghi A100, không phải bằng chứng nhóm đã có allocation A100/H200. Nếu chạy H200, sửa setup/host record, preflight và nhãn kết quả theo GPU thực tế. Không suy quyền HPC từ VM `admin@uit-blockchain-server`.
3. **Gate CPU/GPU:** `make e3-gate` hiện build không có CUDA và không truyền `--prover cuda`, nên mặc định chạy **CPU**, dù gọi trên máy A100/H200. Muốn CUDA phải build feature và gọi prover tương ứng; các task này cần command đầy đủ trong môi trường được cấp.
4. **CPU/GPU repetitions và hardware record:** `e4_preflight.py` hiện chạy một batch A/B cho n=8/64, lưu host/topology/SP1/Anvil và timing. Nó chưa tự thu đầy đủ CPU/RAM/GPU driver/CUDA/OS theo T-E2-01, chưa đáp ứng ≥5 lần/cấu hình hoặc tách toàn bộ compressed/wrap timings theo T-E1-03. Cần mở rộng metadata/measurement thay vì coi một preflight là đủ bảng RQ1.
5. **Raw E1:** `make e1-strict` hiện ghi raw vào `results/e1/strict/` trong working tree; yêu cầu mới là raw ngoài Git. Cần launcher/path phù hợp khi chuẩn bị production run, không commit raw/proof lớn.
6. **E3 parallelism:** chỉ Slurm-array song song khi mỗi job có Fabric/Gateway/Anvil/state/ports riêng. Nếu dùng một topology, phải chạy tuần tự. Gate và runs phải cùng source revision; không tạo gate ở commit cũ rồi dùng sau sửa ABI.
7. **Gas theo shipment:** T-E3-01 đo `G_a` cho **cả epoch**; Table RQ3 lại ghi **gas per shipment**. Với m lot, phải dùng `G_a/m` khi so cùng đơn vị với per-lot baseline, rồi mới tính tỷ lệ `m G_v/G_a`. Fabric-only giữ gas/calldata N/A.
8. **E4 protocol:** paper viết screening “select the region” cho confirmation, nhưng grid 27 cell × 10 seed đã định trước. Phải mô tả đúng thiết kế định trước, hoặc thống nhất thay protocol trước khi chạy; không lựa chọn cell sau khi thấy kết quả mà vẫn gọi là thiết kế ban đầu.
9. **E3 saturation:** paper yêu cầu báo cáo run censored/saturated, trong khi formal analyzer yêu cầu đủ 120 run thành công. Cần giữ/status/report toàn bộ run; nếu chưa đủ không báo p-values formal. `--allow-partial` chỉ dùng báo cáo tiến độ, không lấp bảng formal.
10. **Lệnh/tool chưa đầy đủ:** dấu `...`, `<raw>`, `<unique>` trong checklist là placeholder. `--run-id` của E2 hiện cần số nguyên. `/draw` và `/ready` được nhắc trong todo nhưng không phải lệnh repo hiện có đã được xác minh. `main_marked.tex` được nhắc ở comment nhưng không có file; bibliography trong `main.tex` đang embedded, sửa `.bib` cần đồng bộ phần này.

Các điểm trên là ghi chú để thực thi đúng checklist; lần review này giữ nguyên tài nguyên gốc và chưa đổi runner/contract.

## 5. Thứ tự và phân công thầy đề xuất

| Mốc ghi trong todo-list | Công việc | Người được đề xuất |
|---|---|---|
| 05–09/10 | B1–B3, chốt ABI ngày 09/10, topology/host | Thịnh |
| 08–10/10 | E1 production FRI + wrapper; nhánh Path B độc lập ABI SP1 | Nhân |
| 10–12/10 | E3 gate + CPU/GPU A/B | Trí |
| 12–16/10 | E2 full, mapper, selective disclosure | Thịnh / Nhân |
| 13–23/10 | E3 120 run + gas → fiat | Trí |
| 20–28/10 | E4 screening/confirmation + figure | Nhân |
| 28–30/10 | Điền paper, credits, review, final gate và submit | Thầy |

Đây là phân công/ước tính trong tài liệu, chưa phải issue assignment mới hoặc cam kết runtime đã kiểm chứng. Critical path cần bao gồm **cả B1, B2 và B3** trước khi đóng ABI và đo chính thức, dù dòng mũi tên đầu todo chỉ ghi T-SEC-01. Path B E1 có thể chuẩn bị riêng, nhưng phải chốt cấu hình/semantics dùng cho bảng.

Thứ tự nên làm: sửa B1–B3 và tests → chốt guest/ABI/revision → topology + hardware record → gate/preflight thật → E1/SP1/E2/E3 → E4 → điền paper bằng evidence. Mỗi benchmark giữ raw/log/proof ngoài Git, commit metadata/checksum/summary và đường dẫn artifact có thể truy cập để reviewer đối chiếu.

Nếu compute trễ, checklist cho phép chỉ báo cáo E4 screening với caveat một seed. Nếu thiếu E3 formal, bỏ claim/bảng comparison và chỉ giữ cost model với `G_v`, `G_a` đã đo từ gate. Không thay formal results bằng mock/pilot/accelerated data.

## 6. Kiểm tra trong lần review

- Đã đọc `main.tex`, `todo-list.tex`, `references.bib`, source/style sơ đồ; render và xem đủ năm trang todo-list PDF cùng sơ đồ architecture PDF.
- Đếm 20 task, 19 ID màu đỏ/33 vị trí và 29 mục bibliography; mọi ID đỏ đều có task tương ứng.
- Đối chiếu trực tiếp policy/epoch ABI, contracts, FRI parameters, Makefile, preflight, E3 analyzer, E4 plan và E1/E2 reports.
- Không chạy benchmark, chưa compile `main.tex` (máy chưa có LaTeX compiler trong PATH), chưa xác minh page count của paper tổng hoặc đọc độc lập toàn bộ 29 nguồn bên ngoài.
