use anyhow::Result;
use clap::Parser;
use e2_bench::l1_adapter::create_l1_adapter;
use e2_bench::pipeline::run_pipeline_seed;
use e2_bench::stats::{compute_summary, write_metadata_json, write_raw_csv, write_summary_csv};
use e2_bench::types::{E2Options, L1Mode, ProfileMode};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "e2-bench",
    author = "HuxTris",
    version = "0.1.0",
    about = "E2 End-to-End Latency Benchmark for EPCIS-Aware ZK-Rollup"
)]
struct Cli {
    /// Benchmark profile mode: quick or full
    #[arg(long, value_enum, default_value_t = ProfileMode::Quick)]
    profile: ProfileMode,

    /// Poisson arrival rate lambda (events/min)
    #[arg(long, default_value_t = 480.0)]
    lambda_events_per_min: f64,

    /// Duration in minutes (defaults to 5 for quick, 60 for full)
    #[arg(long)]
    duration_min: Option<f64>,

    /// Number of random seeds (defaults to 3 for quick, 30 for full)
    #[arg(long)]
    seeds: Option<usize>,

    /// L1 confirmation adapter mode: mock, local, anvil, sepolia
    #[arg(long, value_enum, default_value_t = L1Mode::Mock)]
    l1_mode: L1Mode,

    /// Path to write raw latency CSV
    #[arg(long, default_value = "results/e2_latency_raw.csv")]
    out: PathBuf,

    /// Path to write summary CSV
    #[arg(long)]
    summary_out: Option<PathBuf>,

    /// Path to write metadata JSON
    #[arg(long)]
    metadata_out: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let duration_min = cli.duration_min.unwrap_or_else(|| match cli.profile {
        ProfileMode::Quick => 5.0,
        ProfileMode::Full => 60.0,
    });

    let seeds = cli.seeds.unwrap_or_else(|| match cli.profile {
        ProfileMode::Quick => 3,
        ProfileMode::Full => 30,
    });

    let parent_dir = cli.out.parent().unwrap_or_else(|| std::path::Path::new("results"));
    let out_summary = cli
        .summary_out
        .unwrap_or_else(|| parent_dir.join("e2_latency_summary.csv"));
    let out_metadata = cli
        .metadata_out
        .unwrap_or_else(|| parent_dir.join("e2_latency_metadata.json"));

    let options = E2Options {
        profile: cli.profile,
        lambda_events_per_min: cli.lambda_events_per_min,
        duration_min,
        seeds,
        l1_mode: cli.l1_mode,
        out_raw: cli.out,
        out_summary,
        out_metadata,
    };

    println!("Starting E2 End-to-End Latency Benchmark...");
    println!("Profile: {}", options.profile);
    println!("Lambda: {} events/min", options.lambda_events_per_min);
    println!("Duration: {} min", options.duration_min);
    println!("Seeds: {}", options.seeds);
    println!("L1 Mode: {}", options.l1_mode);
    println!("Output Raw: {}", options.out_raw.display());

    let l1_adapter = create_l1_adapter(options.l1_mode);
    let mut all_rows = Vec::new();

    for seed in 1..=options.seeds {
        println!("Running seed {}/{}...", seed, options.seeds);
        let seed_rows = run_pipeline_seed(seed, &options, l1_adapter.as_ref())?;
        all_rows.extend(seed_rows);
    }

    println!("Computing latency statistics across {} events...", all_rows.len());
    let summary = compute_summary(&all_rows, &options);

    write_raw_csv(&all_rows, &options.out_raw)?;
    println!("Wrote raw CSV: {}", options.out_raw.display());

    write_summary_csv(&summary, &options.out_summary)?;
    println!("Wrote summary CSV: {}", options.out_summary.display());

    write_metadata_json(&summary, &options, &options.out_metadata)?;
    println!("Wrote metadata JSON: {}", options.out_metadata.display());

    println!("\n=== E2 Latency Benchmark Summary ===");
    println!("Total Events: {}", summary.total_events);
    println!("Total Lots:   {}", summary.total_lots);
    println!("Median (ms):  {:.2}", summary.median_ms);
    println!("P95 (ms):     {:.2}", summary.p95_ms);
    println!("P99 (ms):     {:.2}", summary.p99_ms);
    println!("Min (ms):     {:.2}", summary.min_ms);
    println!("Max (ms):     {:.2}", summary.max_ms);
    println!("Mean (ms):    {:.2}", summary.mean_ms);

    Ok(())
}
