use anyhow::Result;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct SmokeMetric {
    pub circuit: String,
    pub events_per_lot: usize,
    pub seed: u64,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
    pub recursive_prove_ms: f64,
    pub recursive_verify_ms: f64,
    pub recursive_proof_bytes: usize,
}

pub fn csv_header() -> &'static str {
    "circuit,events_per_lot,seed,prove_ms,verify_ms,proof_bytes,recursive_prove_ms,recursive_verify_ms,recursive_proof_bytes"
}

impl SmokeMetric {
    pub fn to_csv_row(&self) -> String {
        format!(
            "{},{},{},{:.3},{:.3},{},{:.3},{:.3},{}",
            self.circuit,
            self.events_per_lot,
            self.seed,
            self.prove_ms,
            self.verify_ms,
            self.proof_bytes,
            self.recursive_prove_ms,
            self.recursive_verify_ms,
            self.recursive_proof_bytes
        )
    }
}

pub fn write_csv(path: impl AsRef<Path>, rows: &[SmokeMetric]) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = File::create(path)?;
    writeln!(file, "{}", csv_header())?;
    for row in rows {
        writeln!(file, "{}", row.to_csv_row())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_header_matches_week1_plan() {
        assert_eq!(
            csv_header(),
            "circuit,events_per_lot,seed,prove_ms,verify_ms,proof_bytes,recursive_prove_ms,recursive_verify_ms,recursive_proof_bytes"
        );
    }

    #[test]
    fn csv_row_contains_all_smoke_fields() {
        let row = SmokeMetric {
            circuit: "threshold-lite".to_string(),
            events_per_lot: 8,
            seed: 0,
            prove_ms: 1.25,
            verify_ms: 0.50,
            proof_bytes: 128,
            recursive_prove_ms: 2.25,
            recursive_verify_ms: 0.75,
            recursive_proof_bytes: 256,
        };

        assert_eq!(
            row.to_csv_row(),
            "threshold-lite,8,0,1.250,0.500,128,2.250,0.750,256"
        );
    }
}
