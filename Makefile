.PHONY: bootstrap test e1 e1-quick figs clean-results

bootstrap:
	@bash scripts/bootstrap.sh

test:
	cargo test --workspace

e1:
	cargo run --release -p e1-bench -- --out results/e1/raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1
	python3 scripts/e1_analyze.py --raw results/e1/raw.csv --out-dir results/e1

e1-quick:
	cargo run --release -p e1-bench -- --out results/e1/raw.csv --events 8 --seeds 1 --circuits all --profile coffee-small --jobs 1
	python3 scripts/e1_analyze.py --raw results/e1/raw.csv --out-dir results/e1

figs:
	python3 scripts/e1_analyze.py --raw results/e1/raw.csv --out-dir results/e1

clean-results:
	rm -rf results/e1
