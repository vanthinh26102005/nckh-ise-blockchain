use crate::threshold_lite::{E1Config, E1Field, ThresholdProofResult, D};
use anyhow::Result;
use plonky2::iop::witness::{PartialWitness, WitnessWrite};
use plonky2::plonk::circuit_builder::CircuitBuilder;
use plonky2::plonk::circuit_data::{CircuitConfig, CircuitData};
use plonky2::plonk::proof::ProofWithPublicInputs;
use std::fmt;
use std::time::Instant;

pub type RecursiveCircuitData = CircuitData<E1Field, E1Config, D>;
pub type RecursiveProof = ProofWithPublicInputs<E1Field, E1Config, D>;

pub struct RecursiveProofResult {
    pub proof: RecursiveProof,
    pub data: RecursiveCircuitData,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
}

impl fmt::Debug for RecursiveProofResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecursiveProofResult")
            .field("prove_ms", &self.prove_ms)
            .field("verify_ms", &self.verify_ms)
            .field("proof_bytes", &self.proof_bytes)
            .finish_non_exhaustive()
    }
}

impl RecursiveProofResult {
    pub fn public_input_count(&self) -> usize {
        self.proof.public_inputs.len()
    }

    pub fn circuit_degree_bits(&self) -> usize {
        self.data.common.degree_bits()
    }
}

pub fn prove_recursive_wrapper(inner: &ThresholdProofResult) -> Result<RecursiveProofResult> {
    let config = CircuitConfig::standard_recursion_config();
    let mut builder = CircuitBuilder::<E1Field, D>::new(config);
    let proof_target = builder.add_virtual_proof_with_pis(&inner.data.common);
    let verifier_data_target = builder.add_virtual_verifier_data(
        inner.data.common.config.fri_config.cap_height,
    );

    builder.verify_proof::<E1Config>(
        &proof_target,
        &verifier_data_target,
        &inner.data.common,
    );

    let data = builder.build::<E1Config>();
    let mut witness = PartialWitness::new();
    witness.set_proof_with_pis_target(&proof_target, &inner.proof)?;
    witness.set_verifier_data_target(&verifier_data_target, &inner.data.verifier_only)?;

    let prove_start = Instant::now();
    let proof = data.prove(witness)?;
    let prove_ms = prove_start.elapsed().as_secs_f64() * 1_000.0;

    let proof_bytes = proof.to_bytes().len();
    let verify_start = Instant::now();
    data.verify(proof.clone())?;
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1_000.0;

    Ok(RecursiveProofResult {
        proof,
        data,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold_lite::{prove_threshold_trace, ThresholdTrace};

    #[test]
    fn recursive_wrapper_verifies_inner_threshold_proof() {
        let trace = ThresholdTrace::synthetic(8, 0);
        let inner = prove_threshold_trace(&trace).expect("inner proof should verify");
        let recursive = prove_recursive_wrapper(&inner).expect("recursive proof should verify");

        assert!(recursive.proof_bytes > 0);
        assert!(recursive.prove_ms > 0.0);
        assert!(recursive.verify_ms >= 0.0);
    }
}
