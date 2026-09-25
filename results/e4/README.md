# E4 — Scalability & policy trade-off (proposed system)

Xem phương pháp: [`docs/reports/e4_methodology.md`](../../docs/reports/e4_methodology.md) · Issue #22.

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

# 3) Chạy chính — CHỈ sau khi E3 aggregate proof gate pass (executor real-e3)
#    RealE3Executor trong scripts/e4_run.py hiện là stub, chờ cắm pipeline E3.
```

Executor `mock` chỉ để kiểm thử harness/schema/analysis. Số liệu E4 chính thức yêu cầu
`--executor real-e3` và analyzer chạy với `--require-real --require-single-host`.
