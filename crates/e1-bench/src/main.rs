use anyhow::{anyhow, bail, Context, Result};
use e1_bench::{run_benchmark, BenchmarkOptions, CircuitKind, Profile};
use std::path::PathBuf;

fn main() -> Result<()> {
    let options = parse_args()?;
    run_benchmark(options)
}

fn parse_args() -> Result<BenchmarkOptions> {
    let mut out = None;
    let mut events = None;
    let mut seeds = None;
    let mut circuits = None;
    let mut profile = Profile::CoffeeDefault;
    let mut jobs = 1usize;
    let mut include_placeholders = false;
    let mut strict_output = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(args.next().context("--out requires a path")?)),
            "--events" => {
                events = Some(parse_events(
                    &args.next().context("--events requires a CSV list")?,
                )?)
            }
            "--seeds" => seeds = Some(args.next().context("--seeds requires a number")?.parse()?),
            "--circuits" => {
                circuits = Some(parse_circuits(
                    &args.next().context("--circuits requires a CSV list")?,
                )?)
            }
            "--profile" => {
                let raw = args.next().context("--profile requires a value")?;
                profile = Profile::parse(&raw).ok_or_else(|| {
                    anyhow!("unknown profile '{raw}', expected coffee-small,coffee-default,stress")
                })?;
            }
            "--jobs" => jobs = args.next().context("--jobs requires a number")?.parse()?,
            "--include-placeholders" => include_placeholders = true,
            "--strict-output" => strict_output = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let final_circuits = match circuits {
        Some(mut selected) => {
            if include_placeholders && selected == CircuitKind::all(false) {
                selected = CircuitKind::all(true);
            }
            selected
        }
        None => CircuitKind::all(include_placeholders),
    };

    Ok(BenchmarkOptions {
        out: out.unwrap_or_else(|| PathBuf::from("results/e1/raw.csv")),
        events_per_lot: events.unwrap_or_else(|| vec![8, 16, 32, 64]),
        seeds: seeds.unwrap_or(30),
        circuits: final_circuits,
        profile,
        jobs,
        include_placeholders,
        strict_output,
    })
}

fn parse_events(raw: &str) -> Result<Vec<usize>> {
    raw.split(',')
        .map(|v| {
            let parsed = v.trim().parse::<usize>()?;
            if parsed == 0 {
                bail!("events/lot must be positive");
            }
            Ok(parsed)
        })
        .collect()
}

fn parse_circuits(raw: &str) -> Result<Vec<CircuitKind>> {
    if raw.trim().eq_ignore_ascii_case("all") {
        return Ok(CircuitKind::all(false));
    }

    raw.split(',')
        .map(|v| {
            CircuitKind::parse(v.trim()).ok_or_else(|| {
                anyhow!("unknown circuit '{v}', expected c1,c2,c3,c4,c5,wrapper,all")
            })
        })
        .collect()
}

fn print_help() {
    println!(
        "Usage: e1-bench --out results/e1/raw.csv --events 8,16,32,64 --seeds 30 --circuits all --profile coffee-default --jobs 1 [--include-placeholders] [--strict-output]"
    );
}
