use plonky2::field::types::Field;
use plonky2::plonk::config::{GenericConfig, PoseidonGoldilocksConfig};
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

pub const D: usize = 2;
pub type C = PoseidonGoldilocksConfig;
pub type F = <C as GenericConfig<D>>::F;

pub const THRESHOLD: u64 = 900;
pub const RANGE_BITS: usize = 32;
pub const SCHNORR_G: u64 = 7;
pub const POSEIDON_TAG_CERT: u64 = 2;
pub const POSEIDON_TAG_NULLIFIER: u64 = 5;
pub const POSEIDON_TAG_POLYGON: u64 = 11;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CircuitKind {
    C1,
    C2,
    C3,
    C4,
    C5,
    Wrapper,
    C1Legacy,
    C4Legacy,
}

impl CircuitKind {
    pub fn all(include_placeholders: bool) -> Vec<Self> {
        let mut circuits = vec![
            Self::C1,
            Self::C2,
            Self::C3,
            Self::C4,
            Self::C5,
            Self::Wrapper,
        ];
        if include_placeholders {
            circuits.extend([Self::C1Legacy, Self::C4Legacy]);
        }
        circuits
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "c1" | "geofence" | "polygon" => Some(Self::C1),
            "c2" | "certificate" | "merkle" => Some(Self::C2),
            "c3" | "threshold" => Some(Self::C3),
            "c4" | "signature" | "schnorr" => Some(Self::C4),
            "c5" | "nullifier" => Some(Self::C5),
            "wrapper" | "recursive" => Some(Self::Wrapper),
            "c1-legacy" | "bbox" => Some(Self::C1Legacy),
            "c4-legacy" | "signature-commitment" => Some(Self::C4Legacy),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::C1 => "c1_polygon_halfplane",
            Self::C2 => "c2_poseidon_merkle",
            Self::C3 => "c3_threshold_time",
            Self::C4 => "c4_schnorr_proxy",
            Self::C5 => "c5_poseidon_nullifier_set",
            Self::Wrapper => "wrapper_recursive_plonky2",
            Self::C1Legacy => "c1_legacy_bbox",
            Self::C4Legacy => "c4_legacy_signature_commitment",
        }
    }

    pub fn version(self) -> &'static str {
        match self {
            Self::C1 => "v2-real-convex-polygon",
            Self::C2 => "v2-real-poseidon-merkle",
            Self::C3 => "v2-real-threshold-time",
            Self::C4 => "v2-proxy-schnorr-field",
            Self::C5 => "v2-real-nullifier-commitment",
            Self::Wrapper => "v2-real-recursive-verifier",
            Self::C1Legacy | Self::C4Legacy => "v1-placeholder-compat",
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

    pub fn polygon(self) -> Vec<(u64, u64)> {
        match self {
            Self::CoffeeSmall => vec![
                (900, 2100),
                (1200, 1900),
                (1600, 2200),
                (1450, 2600),
                (980, 2550),
            ],
            Self::CoffeeDefault => vec![
                (850, 2050),
                (1160, 1840),
                (1660, 2100),
                (1580, 2650),
                (980, 2720),
            ],
            Self::Stress => vec![
                (760, 2050),
                (1050, 1760),
                (1640, 1840),
                (1780, 2350),
                (1450, 2840),
                (880, 2750),
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
}

#[derive(Serialize)]
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
    F::from_canonical_u64(value)
}

pub fn signed_f(value: i64) -> F {
    if value >= 0 {
        f(value as u64)
    } else {
        -f((-value) as u64)
    }
}
