.PHONY: bootstrap test e1 e1-quick e1-strict e2 e2-quick e2-strict figs clean-results clean-e2 \
	e3-check e3-build e3-build-cuda e3-gate e3-run e3-analyze \
	e4-plan e4-preflight e4-run-mock e4-analyze-mock e4-run e4-merge e4-analyze e4-test clean-e4

bootstrap:
	@bash scripts/bootstrap.sh

test:
	cargo test --workspace

e1:
	@mkdir -p results/e1/full
	cargo run --release -p e1-bench -- --out results/e1/full/raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1
	python3 scripts/e1_analyze.py --raw results/e1/full/raw.csv --out-dir results/e1/full

e1-quick:
	@mkdir -p results/e1/quick
	cargo run --release -p e1-bench -- --out results/e1/quick/raw.csv --events 8 --seeds 1 --circuits all --profile coffee-small --jobs 1 --strict-output
	python3 scripts/e1_analyze.py --raw results/e1/quick/raw.csv --table-out results/e1/quick/summary.csv --plots-out results/e1/quick/plots.png --metadata-out results/e1/quick/metadata.json --report-out results/e1/quick/report.md --expected-seeds 1 --command "make e1-quick"

e1-strict:
	@mkdir -p results/e1/strict
	cargo run --release -p e1-bench -- --out results/e1/strict/raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1 --strict-output
	python3 scripts/e1_analyze.py --raw results/e1/strict/raw.csv --table-out results/e1/strict/summary.csv --plots-out results/e1/strict/plots.png --metadata-out results/e1/strict/metadata.json --report-out results/e1/strict/report.md --expected-seeds 30 --command "make e1-strict"

e2-quick:
	@mkdir -p results/e2/quick
	cargo run --release -p e2-bench -- --profile quick --lambda-events-per-min 480 --duration-min 5 --seeds 3 --l1-mode mock --out results/e2/quick/raw.csv --summary-out results/e2/quick/summary.csv --metadata-out results/e2/quick/metadata.json
	python3 scripts/e2_analyze.py --raw results/e2/quick/raw.csv --out-dir results/e2/quick --cdf-out results/e2/quick/cdf.png --report-out results/e2/quick/report.md

e2:
	@mkdir -p results/e2/full
	cargo run --release -p e2-bench -- --profile full --lambda-events-per-min 480 --duration-min 60 --seeds 30 --l1-mode mock --out results/e2/full/raw.csv --summary-out results/e2/full/summary.csv --metadata-out results/e2/full/metadata.json
	python3 scripts/e2_analyze.py --raw results/e2/full/raw.csv --out-dir results/e2/full --cdf-out results/e2/full/cdf.png --report-out results/e2/full/report.md

e2-strict:
	@mkdir -p results/e2/strict
	cargo run --release -p e2-bench -- --profile full --lambda-events-per-min 480 --duration-min 60 --seeds 30 --l1-mode anvil --out results/e2/strict/raw.csv --summary-out results/e2/strict/summary.csv --metadata-out results/e2/strict/metadata.json
	python3 scripts/e2_analyze.py --raw results/e2/strict/raw.csv --out-dir results/e2/strict --cdf-out results/e2/strict/cdf.png --report-out results/e2/strict/report.md

figs:
	python3 scripts/e1_analyze.py --raw results/e1/quick/raw.csv --table-out results/e1/quick/summary.csv --plots-out results/e1/quick/plots.png --metadata-out results/e1/quick/metadata.json --report-out results/e1/quick/report.md
	python3 scripts/e2_analyze.py --raw results/e2/quick/raw.csv --out-dir results/e2/quick --cdf-out results/e2/quick/cdf.png --report-out results/e2/quick/report.md

clean-results:
	rm -rf results/e1/quick results/e1/strict results/e1/full results/e2/quick results/e2/strict results/e2/full

clean-e2:
	rm -rf results/e2/quick results/e2/strict results/e2/full

# ---- E3 recursive epoch gate and E4 proposed-system scalability ----

e3-check:
	cargo test --offline --manifest-path crates/sp1-e2e/Cargo.toml -p eudr-policy
	cargo check --offline --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e
	cargo test --offline --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e4_real_cell
	python3 scripts/test_e3.py
	node --check contracts/test/anvil_e3_smoke.mjs
	node --check contracts/test/anvil_e3_vecro_smoke.mjs

e3-build:
	cargo build --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e3_epoch_gate

e3-build-cuda:
	cargo build --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e3_epoch_gate --bin e4_real_cell --features cuda

# Start Fabric Gateway + Anvil first. Supply both paths outside the repository.
e3-gate: e3-build
	@test -n "$(E3_FIXTURE)" && test -n "$(E3_RECEIPT)" || (echo 'Set E3_FIXTURE and E3_RECEIPT to output paths outside Git' >&2; exit 2)
	crates/sp1-e2e/target/release/e3_epoch_gate --out "$(abspath $(E3_FIXTURE))"
	cd contracts && E3_FIXTURE="$(abspath $(E3_FIXTURE))" npm run --silent anvil:e3-smoke > "$(abspath $(E3_RECEIPT))"
	python3 scripts/e3_gate_manifest.py --fixture "$(E3_FIXTURE)" --receipt "$(E3_RECEIPT)" \
		--out results/e3/gate_manifest.json --git-commit "$(shell git rev-parse HEAD)"

# One immutable E3 mode/seed per invocation, typically submitted as a Slurm array.
E3_PROVER ?= cuda
E3_WALL_TIME_S ?= 7200
e3-run:
	@test -n "$(E3_MODE)" && test -n "$(E3_SEED)" && test -n "$(E3_RAW_ROOT)" || (echo 'Set E3_MODE, E3_SEED, E3_RAW_ROOT' >&2; exit 2)
	python3 scripts/e3_run.py --binary crates/sp1-e2e/target/release/e4_real_cell \
		--gate results/e3/gate_manifest.json --mode "$(E3_MODE)" --seed "$(E3_SEED)" \
		--prover "$(E3_PROVER)" --wall-time-s "$(E3_WALL_TIME_S)" \
		--out "$(E3_RAW_ROOT)/$(E3_MODE)-$(E3_SEED).json" --git-commit "$(shell git rev-parse HEAD)"

e3-analyze:
	@test -n "$(E3_RAW_ROOT)" || (echo 'Set E3_RAW_ROOT' >&2; exit 2)
	python3 scripts/e3_analyze.py --raw-dir "$(E3_RAW_ROOT)" --out results/e3/comparison.json

e4-test:
	python3 scripts/test_e4.py
	cargo test --offline --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e4_real_cell

e4-plan:
	python3 scripts/e4_plan.py --phase all --out results/e4/plan.csv --summary-out results/e4/plan.summary.json

E4_EPOCH_BASE ?= $(shell date -u +%s)
e4-preflight: e3-build-cuda
	@test -n "$(E4_PREFLIGHT_RAW_ROOT)" && test -n "$(E4_PREFLIGHT)" || (echo 'Set E4_PREFLIGHT_RAW_ROOT and E4_PREFLIGHT' >&2; exit 2)
	python3 scripts/e4_preflight.py --binary crates/sp1-e2e/target/release/e3_epoch_gate \
		--raw-dir "$(E4_PREFLIGHT_RAW_ROOT)" --out "$(E4_PREFLIGHT)" \
		--epoch-base "$(E4_EPOCH_BASE)" --git-commit "$(shell git rev-parse HEAD)"

# Harness/schema/analysis self-test via the mock executor. NOT valid E4 evidence.
e4-run-mock: e4-plan
	@mkdir -p results/e4/screening
	python3 scripts/e4_run.py --plan results/e4/plan.csv --phase screening --executor mock \
		--out results/e4/screening/raw.jsonl --manifest-out results/e4/screening/manifest.json \
		--created-at "$(shell date -u +%Y-%m-%dT%H:%M:%SZ)" --git-commit "$(shell git rev-parse --short HEAD)" \
		--command "make e4-run-mock"

e4-analyze-mock:
	python3 scripts/e4_analyze.py --raw results/e4/screening/raw.jsonl \
		--manifest results/e4/screening/manifest.json --out-dir results/e4/screening

# Formal E4 needs a passed E3 gate manifest, CPU/GPU preflight, and an output
# directory outside Git. Each invocation records a separate immutable raw run.
E4_PHASE ?= screening
E4_RUN_ID ?= $(if $(E4_PLAN_RUN_ID),$(E4_PLAN_RUN_ID),$(if $(E4_PLAN_INDEX),$(E4_PLAN_INDEX),$(shell date -u +%Y%m%dT%H%M%SZ)))
E4_PROVER ?= cuda
E4_WALL_TIME_S ?= 3600
E4_PLAN_RUN_ID ?=
E4_PLAN_INDEX ?=

e4-run: e4-plan
	@test -n "$(E4_RUN_ROOT)" && test -f "$(E3_GATE_MANIFEST)" && test -f "$(E4_PREFLIGHT)" || (echo 'Set E4_RUN_ROOT, E3_GATE_MANIFEST, E4_PREFLIGHT' >&2; exit 2)
	cargo build --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e --bin e4_real_cell --features cuda
	python3 scripts/e4_run.py --plan results/e4/plan.csv --phase "$(E4_PHASE)" --executor real-e3 \
		$(if $(E4_PLAN_RUN_ID),--run-id "$(E4_PLAN_RUN_ID)",) \
		$(if $(E4_PLAN_INDEX),--plan-index "$(E4_PLAN_INDEX)",) \
		--real-binary crates/sp1-e2e/target/release/e4_real_cell --prover "$(E4_PROVER)" \
		--e3-gate-manifest "$(E3_GATE_MANIFEST)" --preflight-ab "$(E4_PREFLIGHT)" \
		--wall-time-s "$(E4_WALL_TIME_S)" \
		--out "$(E4_RUN_ROOT)/$(E4_PHASE)/$(E4_RUN_ID)/raw.jsonl" \
		--manifest-out "$(E4_RUN_ROOT)/$(E4_PHASE)/$(E4_RUN_ID)/manifest.json" \
		--created-at "$(shell date -u +%Y-%m-%dT%H:%M:%SZ)" --git-commit "$(shell git rev-parse HEAD)" \
		--command "make e4-run E4_PHASE=$(E4_PHASE) E4_RUN_ID=$(E4_RUN_ID)"

e4-merge:
	@test -n "$(E4_RUN_ROOT)" || (echo 'Set E4_RUN_ROOT' >&2; exit 2)
	python3 scripts/e4_merge.py --phase "$(E4_PHASE)" --plan results/e4/plan.csv \
		--shard-root "$(E4_RUN_ROOT)/$(E4_PHASE)" \
		--out "$(E4_RUN_ROOT)/$(E4_PHASE)/merged.jsonl" \
		--manifest-out "results/e4/real/$(E4_PHASE)-manifest.json"

e4-analyze:
	@test -f "$(E4_RAW)" && test -f "$(E4_MANIFEST)" || (echo 'Set E4_RAW and E4_MANIFEST' >&2; exit 2)
	python3 scripts/e4_analyze.py --raw "$(E4_RAW)" \
		--manifest "$(E4_MANIFEST)" --out-dir "results/e4/real/$(E4_RUN_ID)" \
		--require-real --require-single-host

clean-e4:
	rm -rf results/e4/screening results/e4/confirmation results/e4/plan.csv results/e4/plan.summary.json
