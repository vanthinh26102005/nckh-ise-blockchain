# E4 — Scalability & policy trade-off (proposed system)

Xem phương pháp: [`docs/reports/e4_methodology.md`](../../docs/reports/e4_methodology.md) · Issue #22.

**Trạng thái cập nhật 04/10/2026:** đã có executor thật, preflight và shard merge.
Checkout hiện chỉ có plan/summary, chưa có kết quả screening/confirmation GPU
chính thức. Xem [báo cáo E4 cho giảng viên](../../docs/reports/e4_teacher_report.md).

## Bố cục

```
results/e4/
  plan.csv                 # sweep plan (COMMITTED — là config, không phải số đo)
  plan.summary.json        # đếm cell/run mỗi phase (committed)
  screening/
    raw.jsonl              # cell records thô (KHÔNG commit)
    manifest.json          # metadata run (committed)
    summary.csv            # tổng hợp per-cell (committed)
    policy_tradeoff.csv    # bảng trade-off shipment×epoch (committed)
    report.md              # báo cáo, minh bạch censored (committed)
    *.png                  # heatmap + proving plot (selected figures, committed)
  confirmation/
    ... (như trên)
```

## Commit vs. không commit

- **Commit:** `plan.csv`, `manifest.json`, `summary.csv`, `policy_tradeoff.csv`, `report.md`, selected `*.png`.
- **KHÔNG commit:** `raw.jsonl` (và mọi raw CSV/JSONL), Fabric crypto/volumes, proof fixture, Anvil artifacts — bị chặn bởi `.gitignore`.

## Chạy

```bash
# 1) Sinh plan (không cần E3)
make e4-plan

# 2) Self-test toàn tuyến bằng mock executor (KHÔNG phải bằng chứng E4)
make e4-run-mock
make e4-analyze-mock

# 3) Chạy chính — sau E3 gate và preflight CPU/GPU cùng commit/compute host.
#    RealE3Executor đã gọi binary Rust e4_real_cell; không còn là stub.
#    Chọn một E4_PLAN_INDEX hoặc E4_PLAN_RUN_ID mỗi invocation để tạo shard.
#    Đặt E4_RUN_ROOT ngoài Git, E3_GATE_MANIFEST và E4_PREFLIGHT trước khi chạy.
#    Ghép đủ shard của phase bằng e4_merge.py rồi chạy analyzer chính thức.
```

Executor `mock` chỉ để kiểm thử harness/schema/analysis. Số liệu E4 chính thức yêu cầu
`--executor real-e3` và analyzer chạy với `--require-real --require-single-host`.
