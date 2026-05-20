use anyhow::{ensure, Result};
use plonky2::field::types::Field;
use plonky2::iop::target::Target;
use plonky2::iop::witness::{PartialWitness, WitnessWrite};
use plonky2::plonk::circuit_builder::CircuitBuilder;
use plonky2::plonk::circuit_data::{CircuitConfig, CircuitData};
use plonky2::plonk::config::{GenericConfig, PoseidonGoldilocksConfig};
use plonky2::plonk::proof::ProofWithPublicInputs;
use std::fmt;
use std::time::Instant;

pub const D: usize = 2;
pub const READING_BITS: usize = 16;
pub const TIMESTAMP_BITS: usize = 32;

pub type E1Config = PoseidonGoldilocksConfig;
pub type E1Field = <E1Config as GenericConfig<D>>::F;
pub type E1CircuitData = CircuitData<E1Field, E1Config, D>;
pub type E1Proof = ProofWithPublicInputs<E1Field, E1Config, D>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdTrace {
    pub threshold: u64,
    pub readings: Vec<u64>,
    pub timestamps: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdPublicInputs {
    pub threshold: u64,
    pub event_count: usize,
    pub first_timestamp: u64,
    pub last_timestamp: u64,
}

pub struct ThresholdProofResult {
    pub events_per_lot: usize,
    pub public_inputs: ThresholdPublicInputs,
    pub proof: E1Proof,
    pub data: E1CircuitData,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
}

impl fmt::Debug for ThresholdProofResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThresholdProofResult")
            .field("events_per_lot", &self.events_per_lot)
            .field("public_inputs", &self.public_inputs)
            .field("prove_ms", &self.prove_ms)
            .field("verify_ms", &self.verify_ms)
            .field("proof_bytes", &self.proof_bytes)
            .finish_non_exhaustive()
    }
}

struct ThresholdTargets {
    threshold: Target,
    first_timestamp: Target,
    last_timestamp: Target,
    readings: Vec<Target>,
    timestamps: Vec<Target>,
    slacks: Vec<Target>,
    deltas: Vec<Target>,
    delta_minus_ones: Vec<Target>,
}

struct ThresholdCircuit {
    data: E1CircuitData,
    targets: ThresholdTargets,
}

impl ThresholdTrace {
    pub fn synthetic(events_per_lot: usize, seed: u64) -> Self {
        let threshold = 1_000;
        let mut readings = Vec::with_capacity(events_per_lot);
        let mut timestamps = Vec::with_capacity(events_per_lot);
        let mut timestamp = 1_700_000_000 + seed * 1_000;

        for idx in 0..events_per_lot {
            let reading = 600 + ((seed * 37 + idx as u64 * 17) % 300);
            readings.push(reading);
            timestamps.push(timestamp);
            timestamp += 60 + ((seed + idx as u64) % 5);
        }

        Self {
            threshold,
            readings,
            timestamps,
        }
    }

    pub fn event_count(&self) -> usize {
        self.readings.len()
    }

    pub fn public_inputs(&self) -> ThresholdPublicInputs {
        ThresholdPublicInputs {
            threshold: self.threshold,
            event_count: self.event_count(),
            first_timestamp: self.timestamps[0],
            last_timestamp: self.timestamps[self.timestamps.len() - 1],
        }
    }
}

pub fn prove_threshold_trace(trace: &ThresholdTrace) -> Result<ThresholdProofResult> {
    validate_trace(trace)?;
    let circuit = build_threshold_circuit(trace.event_count());
    let mut witness = PartialWitness::new();

    witness.set_target(circuit.targets.threshold, E1Field::from_canonical_u64(trace.threshold))?;
    witness.set_target(
        circuit.targets.first_timestamp,
        E1Field::from_canonical_u64(trace.timestamps[0]),
    )?;
    witness.set_target(
        circuit.targets.last_timestamp,
        E1Field::from_canonical_u64(trace.timestamps[trace.timestamps.len() - 1]),
    )?;

    for idx in 0..trace.event_count() {
        let reading = trace.readings[idx];
        let timestamp = trace.timestamps[idx];
        let slack = trace.threshold - reading;
        witness.set_target(circuit.targets.readings[idx], E1Field::from_canonical_u64(reading))?;
        witness.set_target(
            circuit.targets.timestamps[idx],
            E1Field::from_canonical_u64(timestamp),
        )?;
        witness.set_target(circuit.targets.slacks[idx], E1Field::from_canonical_u64(slack))?;
    }

    for idx in 0..trace.event_count() - 1 {
        let delta = trace.timestamps[idx + 1] - trace.timestamps[idx];
        witness.set_target(circuit.targets.deltas[idx], E1Field::from_canonical_u64(delta))?;
        witness.set_target(
            circuit.targets.delta_minus_ones[idx],
            E1Field::from_canonical_u64(delta - 1),
        )?;
    }

    let prove_start = Instant::now();
    let proof = circuit.data.prove(witness)?;
    let prove_ms = prove_start.elapsed().as_secs_f64() * 1_000.0;

    let proof_bytes = proof.to_bytes().len();
    let verify_start = Instant::now();
    circuit.data.verify(proof.clone())?;
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1_000.0;

    Ok(ThresholdProofResult {
        events_per_lot: trace.event_count(),
        public_inputs: trace.public_inputs(),
        proof,
        data: circuit.data,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn validate_trace(trace: &ThresholdTrace) -> Result<()> {
    ensure!(trace.event_count() >= 2, "trace must contain at least two events");
    ensure!(
        trace.readings.len() == trace.timestamps.len(),
        "readings and timestamps must have the same length"
    );
    ensure!(
        trace.threshold < (1_u64 << READING_BITS),
        "threshold exceeds {} bits",
        READING_BITS
    );

    for (idx, reading) in trace.readings.iter().enumerate() {
        ensure!(
            *reading <= trace.threshold,
            "reading {idx} above threshold: {reading} > {}",
            trace.threshold
        );
        ensure!(
            *reading < (1_u64 << READING_BITS),
            "reading {idx} exceeds {} bits",
            READING_BITS
        );
    }

    for (idx, timestamp) in trace.timestamps.iter().enumerate() {
        ensure!(
            *timestamp < (1_u64 << TIMESTAMP_BITS),
            "timestamp {idx} exceeds {} bits",
            TIMESTAMP_BITS
        );
    }

    for idx in 0..trace.timestamps.len() - 1 {
        ensure!(
            trace.timestamps[idx + 1] > trace.timestamps[idx],
            "timestamps must strictly increase at index {idx}"
        );
        let delta = trace.timestamps[idx + 1] - trace.timestamps[idx];
        ensure!(
            delta < (1_u64 << TIMESTAMP_BITS),
            "timestamp delta {idx} exceeds {} bits",
            TIMESTAMP_BITS
        );
    }

    Ok(())
}

fn build_threshold_circuit(event_count: usize) -> ThresholdCircuit {
    let config = CircuitConfig::standard_recursion_config();
    let mut builder = CircuitBuilder::<E1Field, D>::new(config);
    let threshold = builder.add_virtual_target();
    let event_count_target = builder.constant(E1Field::from_canonical_usize(event_count));
    let first_timestamp = builder.add_virtual_target();
    let last_timestamp = builder.add_virtual_target();

    builder.register_public_input(threshold);
    builder.register_public_input(event_count_target);
    builder.register_public_input(first_timestamp);
    builder.register_public_input(last_timestamp);
    builder.range_check(threshold, READING_BITS);

    let one = builder.constant(E1Field::ONE);
    let mut readings = Vec::with_capacity(event_count);
    let mut timestamps = Vec::with_capacity(event_count);
    let mut slacks = Vec::with_capacity(event_count);
    let mut deltas = Vec::with_capacity(event_count.saturating_sub(1));
    let mut delta_minus_ones = Vec::with_capacity(event_count.saturating_sub(1));

    for _ in 0..event_count {
        let reading = builder.add_virtual_target();
        let timestamp = builder.add_virtual_target();
        let slack = builder.add_virtual_target();
        let reading_plus_slack = builder.add(reading, slack);

        builder.connect(reading_plus_slack, threshold);
        builder.range_check(reading, READING_BITS);
        builder.range_check(slack, READING_BITS);
        builder.range_check(timestamp, TIMESTAMP_BITS);

        readings.push(reading);
        timestamps.push(timestamp);
        slacks.push(slack);
    }

    builder.connect(timestamps[0], first_timestamp);
    builder.connect(timestamps[event_count - 1], last_timestamp);

    for idx in 0..event_count - 1 {
        let delta = builder.add_virtual_target();
        let delta_minus_one = builder.add_virtual_target();
        let positive_delta = builder.add(delta_minus_one, one);
        let expected_next = builder.add(timestamps[idx], delta);

        builder.connect(delta, positive_delta);
        builder.connect(expected_next, timestamps[idx + 1]);
        builder.range_check(delta, TIMESTAMP_BITS);
        builder.range_check(delta_minus_one, TIMESTAMP_BITS);

        deltas.push(delta);
        delta_minus_ones.push(delta_minus_one);
    }

    let data = builder.build::<E1Config>();
    ThresholdCircuit {
        data,
        targets: ThresholdTargets {
            threshold,
            first_timestamp,
            last_timestamp,
            readings,
            timestamps,
            slacks,
            deltas,
            delta_minus_ones,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_trace_proves_and_verifies() {
        let trace = ThresholdTrace::synthetic(8, 0);
        let result = prove_threshold_trace(&trace).expect("valid trace should prove");

        assert_eq!(result.events_per_lot, 8);
        assert!(result.proof_bytes > 0);
        assert_eq!(result.public_inputs.event_count, 8);
    }

    #[test]
    fn reading_above_threshold_is_rejected() {
        let mut trace = ThresholdTrace::synthetic(8, 1);
        trace.readings[3] = trace.threshold + 1;

        let err = prove_threshold_trace(&trace).expect_err("invalid reading must be rejected");
        assert!(err.to_string().contains("above threshold"));
    }

    #[test]
    fn non_increasing_timestamp_is_rejected() {
        let mut trace = ThresholdTrace::synthetic(8, 2);
        trace.timestamps[4] = trace.timestamps[3];

        let err = prove_threshold_trace(&trace).expect_err("timestamp regression must be rejected");
        assert!(err.to_string().contains("strictly increase"));
    }
}
