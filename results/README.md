# Benchmark Results

- [E1 evidence index](e1/README.md): local CPU full-grid base report from PR #32 and its verification limits.
- `e1/quick/`, `e1/strict/`, and `e1/full/` are outputs from current Make targets.
- `e2/quick/`, `e2/strict/`, and `e2/full/` are outputs from current Make targets.
- `*/archive/` preserves historical artifacts and must not be used as current benchmark evidence.

The Makefile creates run directories before writing artifacts. `clean-results` and `clean-e2` remove only current run directories and preserve the archives.
