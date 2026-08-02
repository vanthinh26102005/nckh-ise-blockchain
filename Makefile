.PHONY: bootstrap test e1 e1-quick e1-strict e2 e2-quick e2-strict figs clean-results clean-e2

bootstrap:
	@bash scripts/bootstrap.sh

test:
	cargo test --workspace

e1:
	cargo run --release -p e1-bench -- --out results/e1/raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1
	python3 scripts/e1_analyze.py --raw results/e1/raw.csv --out-dir results/e1

e1-quick:
	cargo run --release -p e1-bench -- --out results/e1_raw.csv --events 8 --seeds 1 --circuits all --profile coffee-small --jobs 1 --strict-output
	python3 scripts/e1_analyze.py --raw results/e1_raw.csv --table-out results/e1_table1.csv --plots-out results/e1_plots.png --metadata-out results/e1_metadata.json --report-out results/e1_report.md --expected-seeds 1 --command "make e1-quick"

e1-strict:
	cargo run --release -p e1-bench -- --out results/e1_raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1 --strict-output
	python3 scripts/e1_analyze.py --raw results/e1_raw.csv --table-out results/e1_table1.csv --plots-out results/e1_plots.png --metadata-out results/e1_metadata.json --report-out results/e1_report.md --expected-seeds 30 --command "make e1-strict"

e2-quick:
	cargo run --release -p e2-bench -- --profile quick --lambda-events-per-min 480 --duration-min 5 --seeds 3 --l1-mode mock --out results/e2/raw.csv
	python3 scripts/e2_analyze.py --raw results/e2/raw.csv --out-dir results/e2

e2:
	cargo run --release -p e2-bench -- --profile full --lambda-events-per-min 480 --duration-min 60 --seeds 30 --l1-mode mock --out results/e2/raw.csv
	python3 scripts/e2_analyze.py --raw results/e2/raw.csv --out-dir results/e2

e2-strict:
	cargo run --release -p e2-bench -- --profile full --lambda-events-per-min 480 --duration-min 60 --seeds 30 --l1-mode anvil --out results/e2/raw.csv
	python3 scripts/e2_analyze.py --raw results/e2/raw.csv --out-dir results/e2

figs:
	python3 scripts/e1_analyze.py --raw results/e1_raw.csv --table-out results/e1_table1.csv --plots-out results/e1_plots.png --metadata-out results/e1_metadata.json --report-out results/e1_report.md
	python3 scripts/e2_analyze.py --raw results/e2/raw.csv --out-dir results/e2

clean-results:
	rm -rf results/e1 results/e2 results/e1_raw.csv results/e1_table1.csv results/e1_plots.png results/e1_metadata.json results/e1_report.md
	rm -f results/e2_latency_raw.csv results/e2_latency_summary.csv results/e2_latency_metadata.json results/e2_latency_report.md results/e2_latency_cdf.png

clean-e2:
	rm -rf results/e2
	rm -f results/e2_latency_raw.csv results/e2_latency_summary.csv results/e2_latency_metadata.json results/e2_latency_report.md results/e2_latency_cdf.png


