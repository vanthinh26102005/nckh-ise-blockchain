# Trí: từ đăng nhập HPC UIT đến submit E1–E4

Ngày chuẩn bị: 06/10/2026. Theo dõi công việc: [E1 #40](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/40), [E2 #41](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/41), [E3 #42](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/42), [E4 #43](https://github.com/vanthinh26102005/nckh-ise-blockchain/issues/43).

**Có, phải clone repo trên HPC** nếu chưa có checkout. Mac clone và server clone là hai bản riêng. Job dùng một commit đã chốt, tạo checkout mới cho từng allocation; không pull/change code giữa lượt đo. Nếu Trí sửa local rồi merge, chọn lại commit và chạy lại gate/preflight cùng source trước benchmark.

Scripts trong PR này đã kiểm tra offline; chưa có allocation, CUDA proof, MIG compatibility hoặc receipt mới từ H200. Local smoke E1–E4 vẫn là checklist độc lập trong bốn issue. Không lấy planner/mock/guest execute làm formal evidence.

## 1. Đăng nhập từ Mac ở trường

PDF `Huong_dan_HPC_dungtrt.pdf` ghi: trong mạng UIT SSH trực tiếp; ngoài UIT cần VPN. Dùng tài khoản/key được thầy cấp. VM `admin@10.102.196.203` là hệ thống khác, không phải login node HPC này.

Nếu OpenSSH key đã được chuyển từ PPK và lưu trên Mac ở `~/.ssh/uit_hpc`:

```sh
chmod 600 ~/.ssh/uit_hpc
ssh -i ~/.ssh/uit_hpc -o IdentitiesOnly=yes dungtrt@hpc.uit.edu.vn
```

Nếu chỉ có `.ppk`, cần chuyển sang OpenSSH bằng PuTTYgen trước; không đưa private key lên GitHub hoặc lên compute node. Passphrase lấy riêng từ thầy, không ghi vào report. Nếu DNS/SSH timeout, xử lý kết nối với trường trước; đó chưa phải lỗi proof hay repo.

Các phần sau chạy trong Terminal **đã SSH vào HPC**, trừ bước tải report về Mac. Không train/prove/chạy Fabric trên login node.

Các khối server dùng Bash. Nếu shell đăng nhập khác, chạy `bash` trước; `read -r -p` trong hướng dẫn là cú pháp Bash.

## 2. Kiểm tra scheduler và quyền tài nguyên (login node, read-only)

```sh
hostname
whoami
command -v git python3 sbatch srun squeue sinfo scontrol sacct
python3 --version
sinfo -o '%P %a %l %D %t %G'
sinfo -N -o '%N %P %T %G %E'
sinfo -R
scontrol show partition
squeue -u "$USER"
sacctmgr show assoc where user="$USER" format=Cluster,Account,Partition,QOS
sacctmgr -n -P show assoc where user="$USER" format=Cluster,Account,Partition,QOS%100,DefaultQOS
df -h "/datastore/$USER"
quota -s
```

Nếu `sacctmgr`/quota không được cấp quyền hoặc chưa có, hỏi admin; không thay bằng account tự đoán. `DRAIN`/`DOWN`/reservation là lý do chưa được cấp node, không sửa trạng thái node.

PDF của trường cung cấp ví dụ **small / normal / large**, lần lượt MIG `1g.18gb / 2g.35gb / 3g.71gb`; ví dụ account `uit`, tối đa3ngày/job,50jobs submitted/pending và mỗi account chỉ1job chạy đồng thời. Đây là nội dung tài liệu, **phải kiểm tra cấu hình live/quyền account**. Partition `large` cần thầy cho phép. Không mặc định có toàn bộ H200 hoặc35GB MIG đủ cho SP1.

Lưu lại: partition được phép, account/QOS, GPU GRES có count (ví dụ trong PDF `gpu:2g.35gb:1`), NodeName/hostname compute thực tế, CPU/RAM được phép, time limit, Docker và CUDA toolchain. Job generator dùng `--nodelist` và kiểm tra hostname để giữ cùng host với A/B preflight; nếu NodeName khác hostname, nhờ admin xác nhận cách chọn node, không bỏ kiểm tra để chạy đại.

Kiểm tra live ngày 10/10/2026 của account `dungtrt`: cluster `slurm`, account
`uit`, QOS được cấp `mig1-18,mig2-35,mig3-71,mig7-141`, **DefaultQOS trống**.
Với allocation đã chọn `normal` và `gpu:2g.35gb:1`, phải đặt QOS `mig2-35`.
Đây là snapshot quyền truy cập, không xác nhận còn slot hoặc đủ VRAM cho SP1.
Chạy lại các lệnh kiểm tra nếu chọn slice/partition khác; generator không tự đoán QOS.

## 3. Clone và chốt code trong datastore

PDF chỉ định `/datastore/dungtrt` cho code/data/model/log/environment; `/home/dungtrt` chỉ cấu hình nhỏ. Không dùng `/shared` làm nơi ghi output.

```sh
umask 077
export HPC_ROOT="/datastore/$USER/eudr-nckh"
export HPC_REPO="$HPC_ROOT/repo"
mkdir -p "$HPC_ROOT/jobs" "$HPC_ROOT/logs" "$HPC_ROOT/runs" "$HPC_ROOT/cache"
if [ ! -d "$HPC_REPO/.git" ]; then
  git clone https://github.com/vanthinh26102005/nckh-ise-blockchain.git "$HPC_REPO"
fi
cd "$HPC_REPO"
git status --short --branch
git fetch origin
```

Chỉ tiếp tục khi checkout sạch. Không reset/xóa local changes để vượt check. Sau khi PR này và các local fixes cần thiết đã merge:

```sh
git checkout --detach origin/main
export SOURCE_REVISION=$(git rev-parse HEAD)
printf '%s\n' "$SOURCE_REVISION" > "$HPC_ROOT/source-revision.txt"
test -f scripts/hpc_run.py
test -f infra/hpc/config.example.json
```

Nếu review PR trước khi merge, chọn `origin/codex/hpc-job-handoff` thay `origin/main`; commit đó chỉ phục vụ kiểm tra PR. Khi chuyển source cho formal campaign, tạo lại gate/catalog/preflight. Không chạy `git pull` trong detached job workspace.

Repo và runtime cần nằm trên filesystem compute node truy cập được; Slurm không tự chuyển code/key/data từ Mac sang node. Jobs clone commit từ `$HPC_REPO` vào `$HPC_ROOT/runs/JOBID-phase/workspace`, giữ `.git` để provenance hợp lệ.

## 4. Chuẩn bị môi trường

Login node chỉ tải dependency nhỏ/chuẩn bị config nếu trường cho phép. Build SP1, CUDA, Docker images và proof thực hiện trong **allocation Slurm** hoặc nhờ admin chuẩn bị cache. Không dùng `sudo apt install`/cài driver trên node dùng chung.

Trước hết kiểm tra công cụ có sẵn, load module theo hướng dẫn admin (không đoán tên module):

```sh
command -v cargo rustc rustup node npm go docker flock nvcc
if type module >/dev/null 2>&1; then
  module list
  module -t avail 2>&1
fi
rustup toolchain list
rustc --version
cargo --version
node --version
npm --version
go version
```

Cần Python>=3.11, Go tương thích Gateway (`go.mod` hiện1.25), Rust nightly/succinct và SP1 SDK6.4.0 đã khóa; Node/npm, Docker daemon/Compose, CUDA toolkit và NVIDIA driver do trường quản lý. Không nâng SDK/lockfiles để vượt build error. Hỏi admin nếu Docker chỉ có trên compute node hoặc chỉ cấp Apptainer: topology hiện cần Docker, chưa có adapter Apptainer.

Phiên `login01` đã kiểm tra ngày 10/10 chỉ thấy Python3 và `flock` trong nhóm
công cụ trên; không coi clone repo thành công là môi trường đã sẵn sàng. Module
`cuda12.9/toolkit/12.9.1` có trong danh sách live. Nếu admin xác nhận toolkit này
cho allocation, load nó trước khi submit và kiểm tra lại trên compute node:

```sh
module load cuda12.9/toolkit/12.9.1
command -v cargo rustc rustup node npm go docker flock nvcc
nvcc --version
```

Rust/SP1, Node/npm, Go và Docker vẫn cần môi trường đã được chuẩn bị theo quyền
của trường. Nếu thiếu, dừng ở kiểm tra môi trường; không submit proof để thử may rủi.

Environment phân tích đặt trong datastore; các package này không chạy proof:

```sh
python3 -m venv "$HPC_ROOT/venv"
. "$HPC_ROOT/venv/bin/activate"
python3 -m pip install pandas numpy scipy matplotlib
python3 -m pip freeze > "$HPC_ROOT/python-packages.txt"
export CARGO_BUILD_JOBS=1
export RAYON_NUM_THREADS=2
export CARGO_HOME="$HPC_ROOT/cache/cargo"
export GOMODCACHE="$HPC_ROOT/cache/go-mod"
export npm_config_cache="$HPC_ROOT/cache/npm"
export SP1_GROTH16_CIRCUIT_PATH="$HPC_ROOT/cache/sp1-groth16"
export SP1_PLONK_CIRCUIT_PATH="$HPC_ROOT/cache/sp1-plonk"
```

Giữ environment/module/PATH này khi gọi `sbatch` (default export environment); quay lại phiên SSH mới phải activate/export lại. Các đường circuit trên được SDK6.4.0 hỗ trợ; toolchain/cache khác cần kiểm tra quota, không tự di chuyển thư mục đang tồn tại. Compute cần truy cập cache/package registries hoặc dependency/images đã được admin chuẩn bị. Cold build/setup có thể lâu, tính vào time budget.

Trong một allocation thử do admin chỉ cách cấp, kiểm tra:

```sh
hostname
printf 'CUDA_VISIBLE_DEVICES=%s\n' "${CUDA_VISIBLE_DEVICES:-unset}"
nvidia-smi -L
nvidia-smi --query-gpu=name,driver_version --format=csv
nvcc --version
docker info
docker compose version
```

Không unset GPU visibility do Slurm cấp để dùng GPU khác. Nếu MIG không tương thích backend SP1/CUDA hoặc hết VRAM, giữ log và hỏi admin/thầy về slice phù hợp; có `nvidia-smi` chưa chứng minh CUDA proof chạy được. Nhờ admin cài/pull Foundry image phù hợp compute platform rồi lấy digest:

```sh
docker image inspect ghcr.io/foundry-rs/foundry:stable \
  --format '{{index .RepoDigests 0}}'
```

`anvil_image` trong config dùng digest/image đã được chuẩn bị. Jobs **không tự pull image hoặc xin thêm quyền Docker**. Fabric2.5.16/tool images và Go chaincode build cũng cần sẵn/tải được. Thiếu Docker permission/network/image: dừng ở preflight, gửi error; không đổi qua mock.

## 5. Cấu hình một lần, xem command plan, chưa submit

```sh
cd "$HPC_REPO"
cp infra/hpc/config.example.json "$HPC_ROOT/hpc-config.json"
export HPC_CONFIG="$HPC_ROOT/hpc-config.json"
nano "$HPC_CONFIG"
```

Điền các field `null` từ bước2/4: partition/account/**qos**/gres/compute_host/time/memory_gib/cpus/anvil_image. `qos` bắt buộc và được ghi vào `#SBATCH --qos=…`; thiếu QOS hoặc giá trị không hợp lệ bị từ chối trước khi sinh script. `account=null` chỉ khi admin xác nhận dùng association mặc định; `gres=null` chỉ cho E1 CPU không cần GPU. Các phase đo GPU bắt buộc `prover=cuda` và GPU GRES đã xác nhận. CPU RAM khác GPU VRAM; RAM4GB trong ví dụ train của PDF không phải ngân sách SP1. Chọn đủ RAM theo local/resource review và quota, không coi32/64GB đã được nghiệm thu.

Ví dụ chỉ điền các giá trị đã xác nhận cho slice 35GB; giữ các budget/image/input
khác để review theo allocation thực tế:

```sh
python3 - "$HPC_CONFIG" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1]); c = json.loads(p.read_text())
c.update(partition="normal", account="uit", qos="mig2-35",
         gres="gpu:2g.35gb:1", compute_host="h200node01")
p.write_text(json.dumps(c, indent=2) + "\n")
PY
```

`epoch_base` là ID cho synthetic fixtures/run domain, đổi cho lượt mới để không dùng lại event ID trên ledger cũ. Giữ cùng epoch_base giữa prepare và A/B preflight. Config này chứa **public paths/config, không private key**. `gate/policy_manifest/preflight/pilot_report` được điền sau khi các phase trước có output và đã review. `pilot_reviewed` ban đầufalse.

Xem argv (không build/prove/submit):

```sh
python3 scripts/hpc_run.py --config "$HPC_CONFIG" --phase gate \
  > "$HPC_ROOT/jobs/gate-command-plan.json"
```

Helper dưới **chỉ sinh** script mới, kiểm tra shell; `sbatch` nằm ở bước riêng. `HPC_TIME` phải là budget được scheduler cho phép. Mỗi lần đặt tên script mới để không ghi đè.

```sh
prepare_job() {
  HPC_PHASE="$1"
  HPC_TIME="$2"
  HPC_SCRIPT="$HPC_ROOT/jobs/${HPC_PHASE}-$(date -u +%Y%m%dT%H%M%S).sbatch"
  python3 scripts/hpc_jobs.py --config "$HPC_CONFIG" --phase "$HPC_PHASE" \
    --runtime-root "$HPC_ROOT/runs" --time "$HPC_TIME" --out "$HPC_SCRIPT" || return
  bash -n "$HPC_SCRIPT" || return
  cat "$HPC_SCRIPT"
  export HPC_SCRIPT
}
```

Generator đòi clean committed checkout, full revision, đúng config, real gate khi cần; input file hashes được pin và kiểm tra lại khi job bắt đầu. Mỗi phase khóa topology theo compute host, không array/concurrency. Script không tự submit; không tự claim formalComplete sau exit0.

## 6. Submit gate đầu tiên và đọc kết quả

Chỉ sau local checks khả thi đã đạt; nếu32GB không đủ aggregate, báo rõ resource blocker và dùng allocation ngắn để làm riêng gate này trước campaign dài. Đặt `HPC_TIME` từ quyền thực tế, đủ setup/proving; không copy ví dụ1h của bài train sang benchmark.

```sh
read -r -p 'Time budget gate (HH:MM:SS hoặc D-HH:MM:SS): ' HPC_TIME
prepare_job gate "$HPC_TIME"
```

Đọc script/plan, rồi **mới submit**:

```sh
JOB_ID=$(sbatch --parsable \
  --output="$HPC_ROOT/logs/%x-%j.out" \
  --error="$HPC_ROOT/logs/%x-%j.err" "$HPC_SCRIPT")
JOB_ID=${JOB_ID%%;*}
printf 'JOB_ID=%s\n' "$JOB_ID"
squeue -j "$JOB_ID"
scontrol show job "$JOB_ID"
```

HPC trả job ID nghĩa là đã nhận submit, chưa chắc đã được cấp node. `PD` đọc reason; `R` mới đang chạy. Theo dõi và kiểm tra terminal status:

`QOSMaxJobsPerUserLimit` nghĩa là QOS đã nhận job nhưng account đang chạm giới
hạn số job chạy; bổ sung `--qos` không gỡ giới hạn này. Không hủy job của thầy/nhóm
khác. Dùng `squeue -r -u "$USER"` và `squeue --start -j "$JOB_ID"` để xem lại;
`START_TIME=N/A` chưa có thời điểm chạy dự kiến.

```sh
tail -n 60 "$HPC_ROOT/logs/eudr-gate-$JOB_ID.out"
tail -n 60 "$HPC_ROOT/logs/eudr-gate-$JOB_ID.err"
sacct -j "$JOB_ID" --format=JobID,State,ExitCode,Elapsed,MaxRSS,AllocTRES
export GATE_OUT="$HPC_ROOT/runs/$JOB_ID-gate/output"
cat "$GATE_OUT/job-result.json"
cat "$GATE_OUT/gate-manifest.json"
```

Expected gate: two lots×8events Fabric→compressed proofs→aggregate Groth16; SP1 verifier thật, tampered child/proof/values rejected; receipt status1/root/log/count đúng; manifest source/keys đúng. Không dùng stub/execute để tạo PASS. Thiếu output/OUT_OF_MEMORY/TIMEOUT/failed: chưa chuyển sang benchmark.

Các phase dựng stack (gate, A/B preflight, E2 pilot/formal, E3 và E4) tự gọi
`scripts/e3_local_warmup.py` trước workload đo. Sentinel được commit/readback và
loại khỏi số liệu; kết quả nằm ở `fabric-warmup.json`. Warmup thất bại thì phase
dừng, giữ output/stack để kiểm tra, không chuyển sang measurement.

Điền output mới vào config bằng public paths (không sửa file manifest):

```sh
python3 - "$HPC_CONFIG" "$GATE_OUT" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1]); out = Path(sys.argv[2]); c = json.loads(p.read_text())
c['gate'] = str(out / 'gate-manifest.json')
c['policy_manifest'] = str(out / 'gate-policy.json')
p.write_text(json.dumps(c, indent=2) + '\n')
PY
```

## 7. Prepare catalog rồi A/B preflight

Submit **prepare** với helper ở bước5/6, thay phase. Phase này build/setup và cấp snapshots **trước đo**, không tạo proof: hai workloads A/B8/64,30E3 workloads và350E4 workloads; giữ gate snapshot cùng keys. Có thể mất thời gian; chọn đủ budget/cache. Output mới có `catalog.json`, `e3-plan.csv`, `e4-plan.csv` và350-run plan summary.

```sh
read -r -p 'Time budget chuẩn bị catalog: ' HPC_TIME
prepare_job prepare "$HPC_TIME"
```

Submit/monitor như bước6 và review. Sau prepare thành công, đặt `policy_manifest` trong config tới **`.../JOBID-prepare/output/catalog.json`** (không dùng gate-policy cho toàn campaign). Catalog giữ cố định cho A/B/E3/E4 để digest/snapshot nhất quán.

```sh
read -r -p 'Đường tuyệt đối catalog.json đã review: ' CATALOG
python3 - "$HPC_CONFIG" "$CATALOG" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1]); c = json.loads(p.read_text()); c['policy_manifest'] = sys.argv[2]
p.write_text(json.dumps(c, indent=2) + '\n')
PY
read -r -p 'Time budget A/B preflight: ' HPC_TIME
prepare_job preflight "$HPC_TIME"
```

Submit/monitor riêng. A/B chạy20samples =8/64events×CPU/CUDA×5repetitions, reuse cùng witness, SP1 verification và receipts thật; cùng compute host/source/catalog. Review `.../JOBID-preflight/output/ab-preflight.json`; đặt config `preflight` tới file này trước E4. Không tự điền timings để vượt gate.

## 8. E1–E4: một job tại một thời điểm

Mỗi lần: sửa selectors trong config → xem command plan → `prepare_job PHASE TIME` → đọc `.sbatch` → submit bằng khối ở bước6 → review output trước phase/job tiếp. Thay prefix log/`JOBID-phase` theo phase đang chạy. Không paste liên tiếp nhiều `sbatch` hoặc dùng array vào topology/ports cố định.

| Phase | Config chọn | Job thực sự làm | Output cần review |
|---|---|---|---|
| `e1` | `e1_mode=base`, depth6/16 | C1–C5×8/16/32/64×30seed, strict analyzer, production FRI | `e1/base/depthN/raw.csv` và analyzer metadata/report |
| `e1` | `e1_mode=wrapper`, depth6/16 | Wrapper n8/64×10seed, feature recursion, strict analyzer | `e1/wrapper/depthN/raw.csv` và report |
| `e2-pilot` | gate đã review; CUDA | Manifest E2 riêng, topology provenance,480events/min×1min×1seed accelerated | `e2-policy.json`, `e2-preflight.json`, raw JSONL, `e2-analysis.json` |
| `e2` | `pilot_report` và `pilot_reviewed=true` sau review | Manifest E2 mới giữrunId/schedule;480/min×60min×30seed paced, real runner/analyzer | raw failures/terminal accounting, analysis và receipts |
| `e3` | `e3_seed=0..29`, `e3_mode` theo plan | Một mode/seed,125producers, shipment64,60epochs×60s, fresh ledger | `isolation.json`, `e3-cell.json`, roots/receipt/censor metadata |
| `e4` | `e4_phase=screening/confirmation`, index0..79/0..269 | Một planned real cell,4epochs, gate+A/B preflight đúnghost/source/catalog | `raw.jsonl`, `manifest.json` và receipt metrics |

Ví dụ chỉ sinh script E3, chưa submit:

```sh
python3 scripts/hpc_run.py --config "$HPC_CONFIG" --phase e3 \
  > "$HPC_ROOT/jobs/e3-selected-command-plan.json"
read -r -p 'Time budget E3 cell: ' HPC_TIME
prepare_job e3 "$HPC_TIME"
```

E3 rotate mode order theo seed trong `e3-plan.csv`; đi đúng thứ tự để campaign paired. Không dùng cùng ledger giữa modes. Tổng120cells cần nhiều allocation, không nhét vào một job72h. E4 giữ80screening +270confirmation và4epochs/cell; không chọn lại grid sau khi xem kết quả.

E2 formal tối thiểu30giờ producer + setup/drain/proving; generator từ chối budget dưới30giờ45phút. E3/E4 budget tối thiểu `wall_time_s+2700s` (runtime timeout và reserve setup/collection); đó là **ngưỡng chống job chắc chắn quá ngắn**, không cam kết runtime đủ. Chọn theo pilot/cache và time limit trường. Nếu formal E2 không fit max allocation, giữ evidence incomplete và sửa runner có checkpoint được review trước khi chia campaign; không giảm60min/30seed hoặc đổi sang accelerated.

E2 GPU pilot phải có ít nhất một anchored lot, matching source/host/proverCUDA/axes; review counters/saturation/terminals và runtime trước khi đặt `pilot_reviewed=true`. Chỉ có rawfile hoặc exit0 không đủ chứng minh workload đầy đủ.

## 9. Collect, analyze đầy đủ và gửi report

Giữ toàn bộ failed/censored/saturated records. `job-result.json.executionComplete` là trạng thái orchestration; `formalComplete=false` vẫn giữ cho đến khi campaign coverage/analyzer đạt. Một gate hoặc một cell không đủ paper evidence.

E3: sau120cells, copy **chỉ** `e3-cell.json` sang một directory ngoài Git với tên `MODE-SEED.json`; không overwrite trùng. Rồi:

```sh
python3 scripts/e3_analyze.py --raw-dir "$HPC_ROOT/collected/e3" \
  --out "$HPC_ROOT/collected/e3-comparison.json"
```

Thiếu cells chỉ dùng `--allow-partial` để xem pilot/completion, không p-values formal. E4: giữ mỗi shard `raw.jsonl`+`manifest.json` trong directory riêng dưới `collected/e4/PHASE/`; cùng catalog/gate/preflight và hardware. Chọn plan CSV do prepare tạo, rồi:

```sh
read -r -p 'Đường e4-plan.csv đã review: ' E4_PLAN
read -r -p 'Phase merge (screening/confirmation): ' E4_PHASE
python3 scripts/e4_merge.py --phase "$E4_PHASE" --plan "$E4_PLAN" \
  --shard-root "$HPC_ROOT/collected/e4/$E4_PHASE" \
  --out "$HPC_ROOT/collected/e4/$E4_PHASE-merged.jsonl" \
  --manifest-out "$HPC_ROOT/collected/e4/$E4_PHASE-merged-manifest.json"
python3 scripts/e4_analyze.py \
  --raw "$HPC_ROOT/collected/e4/$E4_PHASE-merged.jsonl" \
  --manifest "$HPC_ROOT/collected/e4/$E4_PHASE-merged-manifest.json" \
  --out-dir "$HPC_ROOT/collected/e4/$E4_PHASE-analysis" \
  --require-real --require-single-host
```

Nếu missing/duplicate/source/host/manifest mismatch, sửa collection hoặc rerun thiếu, không bỏ validator. Gas/calldata báo theo shipment; receipt gas và dated price scenarios chỉ là estimates, không phí mainnet đã trả. PathB circuit costs không tương đương toàn PathA.

Bàn giao cho nhóm: sourceSHA, config public, toolchain/images, `.out/.err`, gate/preflight/manifests, resource report, analyzer summaries và raw checksums/location. **Không upload nguyên runtime:** witness, nullifier/signing keys, Fabric crypto và private disclosure archives giữ riêng. Public report/metadata chọn lọc mới đưa vào PR.

Ví dụ chuẩn bị package public tối thiểu ở server (biến `GATE_OUT` trỏ gate đã review):

```sh
mkdir -p "$HPC_ROOT/public-report"
cp "$HPC_ROOT/source-revision.txt" "$HPC_ROOT/public-report/"
cp "$GATE_OUT/gate-manifest.json" "$GATE_OUT/gate-receipt.json" "$HPC_ROOT/public-report/"
```

Trên **Mac của Trí**, mở Terminal khác và tải đúng directory public đã chọn, không cả `runs/`:

```sh
mkdir -p "$HOME/Documents/NCKH-hpc-reports"
scp -i ~/.ssh/uit_hpc -r \
  dungtrt@hpc.uit.edu.vn:/datastore/dungtrt/eudr-nckh/public-report \
  "$HOME/Documents/NCKH-hpc-reports/"
```

## 10. Khi job lỗi hoặc cần dừng

```sh
scontrol show job "$JOB_ID"
sacct -j "$JOB_ID" --format=JobID,State,ExitCode,Elapsed,MaxRSS,AllocTRES
```

Chỉ hủy job của mình sau khi xác nhận job ID:

```sh
scancel "$JOB_ID"
```

Stack được tắt chỉ sau successful collection; job lỗi giữ topology/outputs để phục hồi. Job tiếp thấy topology còn tồn tại sẽ refuse. Nhờ admin/nhóm kiểm tra ownership và thu log trước cleanup; không `docker system prune`, kill process người khác hoặc fake gate/preflight. Nếu allocation hết hạn, cleanup/recovery cũng phải theo quyền trường, không SSH vào compute để vượt scheduler.

Tham chiếu semantics scheduler: [Slurm sbatch](https://slurm.schedmd.com/sbatch.html), [Slurm sacct](https://slurm.schedmd.com/sacct.html). Tài liệu trường là nguồn storage/access/MIG policy; thông tin live ở bước2/4 mới quyết định config được phép dùng.
