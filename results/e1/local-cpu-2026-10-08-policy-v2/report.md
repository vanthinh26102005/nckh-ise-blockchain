# E1 policy-v2 trên máy local, 08/10/2026

## Kết luận và phạm vi

Lượt chạy này sử dụng source sạch `e6ea9b9eb64b57ce3f922897beba26acbff1e8ea` sau khi sửa lỗi Fabric/Docker #46. Path B C1–C5 đã prove/verify đầy đủ ở `n=8,16,32,64`, credential depth `6,16`, 30 seed cho mỗi ô: **1.200/1.200 hàng `ok`**, 40/40 ô qua strict analyzer. Recursive wrapper Plonky3 đã prove/verify ở `n=8` và `n=64`, depth 16, một seed mỗi cấu hình. Path A SP1 leaf policy v2 cũng đã tạo và verify một Groth16 proof thật cho 8 event. Đây là **evidence local CPU**; `formalComplete=false` vì chưa đủ campaign wrapper 10 seed ở cả hai depth, SP1 CPU/GPU nhiều lượt, hai-lot aggregate và HPC. Không dùng số liệu này làm headline hoàn chỉnh của paper.

Máy: macOS 26.5, Apple M1 Pro arm64, 8 logical CPU, 32 GiB RAM; Rust nightly 1.97.0; Docker Engine 29.2.1. Chi tiết phiên bản và lockfile ở [provenance.json](provenance.json) và [lockfile-sha256.txt](lockfile-sha256.txt). Các phép đo không chạy song song. Log build, run và proof fixture riêng tư nằm ngoài Git tại `/Users/huxtris/Documents/NCKH-local-validation/e1-e4.LNYgZP/`.

## Path B: base C1–C5

Build: `cargo build --locked --release -p e1-bench`. Mỗi depth dùng lệnh sau với `<depth>` bằng 16 hoặc 6 và file output mới:

```sh
./target/release/e1-bench --out <raw.csv> \
  --events 8,16,32,64 --seeds 30 --circuits all \
  --profile coffee-default --credential-depth <depth> --jobs 1 --strict-output
uv run --with pandas --with numpy python scripts/e1_analyze.py \
  --raw <raw.csv> --out-dir <analysis-dir> \
  --expected-seeds 30 --expected-events 8,16,32,64 \
  --expected-depths <depth> \
  --expected-circuits c1_polygon_outside,c2_poseidon2_merkle,c3_threshold_time,c4_poseidon2_actor_authorization,c5_poseidon2_sparse_nullifier_depth32 \
  --command 'local CPU E1 C1-C5 n=8,16,32,64 seeds=30 depth=<depth> coffee-default source=e6ea9b9' \
  --strict --no-plots
```

Tất cả hàng raw và bản thống kê mean + bootstrap 95% CI của analyzer nằm tại [raw/](raw/) và [analysis/](analysis/). [Combined summary](analysis/e1-base-combined-summary.csv) bổ sung median, Q1, Q3 (pandas quantile mặc định, nội suy tuyến tính) để đối chiếu yêu cầu median [IQR]. Bảng dưới là **median**, mỗi ô theo dạng `prove s / verify ms / proof kB` (kB thập phân); bảng CSV chứa cả `n=16,32`.

| Depth | Circuit | n=8 | n=64 |
|---:|---|---:|---:|
| 6 | C1 polygon | 0.560 / 8.09 / 237.2 | 4.207 / 9.61 / 296.0 |
| 6 | C2 Merkle | 0.037 / 3.02 / 110.3 | 0.044 / 3.05 / 110.3 |
| 6 | C3 threshold/time | 0.037 / 2.12 / 57.4 | 0.050 / 2.83 / 82.5 |
| 6 | C4 actor | 0.064 / 2.92 / 100.5 | 0.083 / 2.98 / 100.6 |
| 6 | C5 nullifier | 0.734 / 202.04 / 7494.3 | 0.734 / 202.32 / 7494.7 |
| 16 | C1 polygon | 0.565 / 8.19 / 237.2 | 4.234 / 9.70 / 296.0 |
| 16 | C2 Merkle | 0.065 / 3.35 / 122.4 | 0.043 / 3.43 / 122.4 |
| 16 | C3 threshold/time | 0.036 / 2.13 / 57.4 | 0.050 / 2.83 / 82.5 |
| 16 | C4 actor | 0.064 / 2.96 / 100.5 | 0.085 / 2.97 / 100.6 |
| 16 | C5 nullifier | 0.740 / 203.30 / 7494.3 | 0.744 / 204.27 / 7494.7 |

Watchdog cho toàn run: depth 16 `409.85 s`, peak process-tree RSS `0.856 GiB`; depth 6 `383.64 s`, peak `2.350 GiB`. Trường `peak_ram_gb` trong từng hàng CSV là **process-lifetime peak**, nên không suy diễn là RAM riêng của một circuit. FRI cấu hình log blowup 3, 33 queries, 16 grinding bits, khoảng 115 conjectured bits theo checklist của dự án; đây không phải chứng nhận security. C2 là Poseidon2 membership, C4 là Poseidon2 actor preimage, C5 là sparse nullifier depth 32: Path B không tương đương hash, Ed25519 hoặc nullifier depth 256 của Path A.

## Path B: recursive wrapper

Build: `cargo build --locked --release -p e1-bench --features recursion`. Mỗi lượt dùng `--events 8` hoặc `--events 64`, `--seeds 1 --circuits wrapper --profile coffee-small --credential-depth 16 --jobs 1 --strict-output`; strict analyzer yêu cầu đúng 1 seed, 1 event size, circuit `wrapper_recursive_plonky3`, depth 16. Hai hàng raw có trong [raw/](raw/).

| n | Prove (s) | Verify (ms) | Proof (B) | CSV lifetime peak (GiB) | Watchdog elapsed (s) | Watchdog peak RSS (GiB) |
|---:|---:|---:|---:|---:|---:|---:|
| 8 | 464.129 | 11.784 | 419,020 | 8.973 | 466.670 | 8.972 |
| 64 | 466.356 | 11.725 | 419,016 | 9.227 | 472.196 | 9.205 |

Lượt n=8 đầu dừng sau `310.98 s` vì chạm giới hạn **swap tăng 4 GiB**, không tạo hàng raw. Lượt thử lại dùng giới hạn swap tăng 12 GiB và thành công; cả hai watchdog report được giữ trong [budgets/](budgets/). Không gộp lượt bị dừng vào timing thành công. Một seed không tạo được phân phối, IQR hoặc CI cho wrapper, và wrapper này chưa có EVM verifier.

## Path A: SP1 leaf

Build: `CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 cargo build --locked --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin sp1-e2e`. Chạy `crates/sp1-e2e/target/release/sp1-e2e` với `SP1_FIXTURE_OUT` trỏ tới file **ngoài Git**. Binary gọi `prove_evm_fixture(valid_input())`: một lot 8 event policy v2; nội bộ setup policy prover, chạy Groth16 và xác minh proof cùng ABI public values. CPU prover được chọn tường minh trong source.

Lượt đầu dừng sau **186.77 s** do swap tăng quá 12 GiB, không tạo fixture. Lượt thứ hai dùng giới hạn swap tăng 30 GiB, RSS 28 GiB, wall 3600 s và **thành công**. Đây là cold cache: SP1 tải và giải nén artifact Groth16 v6.1.0 (cache cuối khoảng 7.8 GiB). Vì vậy **2,059.53 s là wall của cả setup + compressed proving + tải cache + Groth16 wrapping + verify**, không phải thời gian proving thuần. Peak process-tree RSS **16.49 GiB**. Log SDK chỉ tách được bước FFI `Generating proof` **47.33 s** (Groth16 wrapping), không tách riêng thời gian compressed proving. Số liệu một lượt không có IQR/CI.

Fixture được giữ ngoài Git có `protocolVersion=2`, `sourceRevision=e6ea9b9eb64b57ce3f922897beba26acbff1e8ea`, `verificationKind=sp1-groth16`, **proof 356 B**, **ABI public values 352 B**. SHA256 fixture là `d1b2aee3e85de1da4e131cbc4279a45c7c7c8b2b85d69987d4edd4135dfd1594`; không đưa raw proof hoặc witness vào repository. [sp1-metrics.json](sp1-metrics.json) ghi các giá trị public, [budgets/](budgets/) giữ báo cáo cả hai lần thử. Chưa có aggregate hai lot hay EVM receipt trong lượt E1 này.

## Đối chiếu và việc còn lại

- [SHA256SUMS](SHA256SUMS) bảo vệ các file public trong bộ dữ liệu này. Metadata analyzer ghi raw SHA256, coverage, status và provenance cho từng lượt. Raw CSV chỉ chứa metrics/provenance, không chứa witness, private key hay proof fixture.
- Fabric 2.5.16 đã khởi động topology 2 peer/3 orderer và cài, approve, commit chaincode thành công trên Docker Engine 29.2.1; log ở thư mục ngoài Git nêu trên. Đây là kiểm tra cho fix #46, không phải kết quả E2.
- Chưa chạy GPU vì máy local M1 Pro không có CUDA/NVIDIA. Còn pending wrapper depth 6 và 10 seed/config, SP1 leaf/aggregate benchmark đủ repetitions, EVM receipt, HPC campaign. Số liệu cũ của PR #32 chưa được xác thực lại từ raw gốc và không trộn với dữ liệu policy v2 này.
