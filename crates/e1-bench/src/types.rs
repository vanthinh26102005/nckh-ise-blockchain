use crate::epcis::PointE6;
use p3_goldilocks::Goldilocks;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub type F = Goldilocks;

pub const THRESHOLD: u64 = 900;
pub const RANGE_BITS: usize = 32;
pub const STRICT_MERKLE_DEPTH: usize = 16;
pub const STRICT_MERKLE_LEAVES: usize = 1 << STRICT_MERKLE_DEPTH;
pub const POSEIDON_TAG_CERT: u64 = 2;
pub const POSEIDON_TAG_ACTOR: u64 = 4;
pub const POSEIDON_TAG_NULLIFIER: u64 = 5;
pub const POSEIDON_TAG_EMPTY: u64 = 6;
pub const POSEIDON_TAG_POLYGON: u64 = 11;
pub const POSEIDON_TAG_EVENT: u64 = 12;
pub const POSEIDON_TAG_EVENT_BATCH: u64 = 13;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CircuitKind {
    C1,
    C2,
    C3,
    C4,
    C5,
    Wrapper,
    C1Legacy,
}

impl CircuitKind {
    pub fn all(include_placeholders: bool) -> Vec<Self> {
        let mut circuits = vec![Self::C1, Self::C2, Self::C3, Self::C4, Self::C5];
        if include_placeholders {
            circuits.push(Self::C1Legacy);
        }
        circuits
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "c1" | "geofence" | "polygon" => Some(Self::C1),
            "c2" | "certificate" | "merkle" => Some(Self::C2),
            "c3" | "threshold" => Some(Self::C3),
            "c4" | "actor" | "authorization" | "auth" => Some(Self::C4),
            "c5" | "nullifier" => Some(Self::C5),
            "wrapper" | "recursive" => Some(Self::Wrapper),
            "c1-legacy" | "bbox" => Some(Self::C1Legacy),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::C1 => "c1_polygon_outside",
            Self::C2 => "c2_poseidon2_merkle_depth16",
            Self::C3 => "c3_threshold_time",
            Self::C4 => "c4_poseidon2_actor_authorization",
            Self::C5 => "c5_poseidon2_nullifier_empty_leaf",
            Self::Wrapper => "wrapper_recursive_plonky3",
            Self::C1Legacy => "c1_legacy_bbox",
        }
    }

    pub fn version(self) -> &'static str {
        match self {
            Self::C1 => "v4-plonky3-wgs84-polygon-poseidon2-commitments",
            Self::C2 => "v4-plonky3-poseidon2-merkle-depth16",
            Self::C3 => "v4-plonky3-threshold-time",
            Self::C4 => "v4-plonky3-poseidon2-actor-authorization",
            Self::C5 => "v4-plonky3-poseidon2-nullifier-empty-leaf",
            Self::Wrapper => "v5-plonky3-recursion-github-blocked",
            Self::C1Legacy => "v1-placeholder-compat",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Profile {
    CoffeeSmall,
    CoffeeDefault,
    Stress,
}

impl Profile {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "coffee-small" => Some(Self::CoffeeSmall),
            "coffee-default" => Some(Self::CoffeeDefault),
            "stress" => Some(Self::Stress),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoffeeSmall => "coffee-small",
            Self::CoffeeDefault => "coffee-default",
            Self::Stress => "stress",
        }
    }

    pub fn previous_nullifier_count(self, events: usize) -> usize {
        match self {
            Self::CoffeeSmall => events.min(8).next_power_of_two(),
            Self::CoffeeDefault => events.min(16).next_power_of_two(),
            Self::Stress => events.next_power_of_two(),
        }
    }

    pub fn polygon(self) -> Vec<PointE6> {
        match self {
            Self::CoffeeSmall => vec![
                PointE6::new(10_760_000, 106_660_000),
                PointE6::new(10_780_000, 106_650_000),
                PointE6::new(10_800_000, 106_680_000),
                PointE6::new(10_785_000, 106_720_000),
                PointE6::new(10_765_000, 106_710_000),
            ],
            Self::CoffeeDefault => vec![
                PointE6::new(10_755_000, 106_655_000),
                PointE6::new(10_780_000, 106_645_000),
                PointE6::new(10_805_000, 106_675_000),
                PointE6::new(10_795_000, 106_725_000),
                PointE6::new(10_760_000, 106_730_000),
            ],
            Self::Stress => vec![
                PointE6::new(10_750_000, 106_655_000),
                PointE6::new(10_775_000, 106_640_000),
                PointE6::new(10_810_000, 106_650_000),
                PointE6::new(10_820_000, 106_690_000),
                PointE6::new(10_790_000, 106_735_000),
                PointE6::new(10_755_000, 106_725_000),
            ],
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct BenchmarkOptions {
    pub out: PathBuf,
    pub events_per_lot: Vec<usize>,
    pub seeds: usize,
    pub circuits: Vec<CircuitKind>,
    pub profile: Profile,
    pub jobs: usize,
    pub include_placeholders: bool,
    pub strict_output: bool,
}

#[derive(Deserialize, Serialize)]
pub struct MetricRow {
    pub seed: usize,
    pub events_per_lot: usize,
    pub circuit: String,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
    pub peak_rss_mb: f64,
    pub status: String,
    pub note: String,
    pub circuit_version: String,
    pub build_ms: f64,
    pub witness_ms: f64,
    pub setup_ms: f64,
    pub gate_count: usize,
    pub public_inputs: usize,
    pub inner_prove_ms: f64,
}

pub fn f(value: u64) -> F {
    F::new(value)
}

pub fn signed_f(value: i64) -> F {
    if value >= 0 {
        f(value as u64)
    } else {
        -f((-value) as u64)
    }
}
