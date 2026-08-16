.PHONY: bootstrap test e1 e1-quick e1-strict e2 e2-quick e2-strict figs clean-results clean-e2

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
