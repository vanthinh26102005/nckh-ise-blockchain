use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileMode {
    Quick,
    Full,
}

impl fmt::Display for ProfileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileMode::Quick => write!(f, "quick"),
            ProfileMode::Full => write!(f, "full"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum L1Mode {
    Mock,
    Local,
    Anvil,
    Sepolia,
}

impl fmt::Display for L1Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            L1Mode::Mock => write!(f, "mock"),
            L1Mode::Local => write!(f, "local"),
            L1Mode::Anvil => write!(f, "anvil"),
            L1Mode::Sepolia => write!(f, "sepolia"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct E2Options {
    pub profile: ProfileMode,
    pub lambda_events_per_min: f64,
    pub duration_min: f64,
    pub seeds: usize,
    pub l1_mode: L1Mode,
    pub out_raw: PathBuf,
    pub out_summary: PathBuf,
    pub out_metadata: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencyRow {
    pub seed: usize,
    pub event_id: usize,
    pub lot_id: usize,
    pub epoch_id: usize,
    pub l1_mode: String,
    pub ingest_at_ms: u64,
    pub lot_ready_at_ms: u64,
    pub proof_start_ms: u64,
    pub proof_end_ms: u64,
    pub submit_tx_at_ms: u64,
    pub confirmed_at_ms: u64,
    pub total_latency_ms: u64,
    pub status: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencySummary {
    pub profile: String,
    pub l1_mode: String,
    pub lambda_events_per_min: f64,
    pub duration_min: f64,
    pub seeds: usize,
    pub total_events: usize,
    pub total_lots: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
}
