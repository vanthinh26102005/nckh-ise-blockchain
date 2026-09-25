# Benchmark Results

- `e1/quick/`, `e1/strict/`, and `e1/full/` are outputs from current Make targets.
- `e2/quick/`, `e2/strict/`, and `e2/full/` are outputs from current Make targets.
- `*/archive/` preserves historical artifacts and must not be used as current benchmark evidence.
- `e3/raw/` contains the canonical-manifest replay records and is ignored by Git. Only validated E3 summaries, metadata, methodology, and analysis code belong in the repository.
- `e3/schema.json` is the committed raw JSONL record schema for the independent E3 runner.

The Makefile creates run directories before writing artifacts. `clean-results` and `clean-e2` remove only current run directories and preserve the archives.
