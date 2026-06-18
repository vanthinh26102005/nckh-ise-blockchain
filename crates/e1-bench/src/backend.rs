use crate::types::F;
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct BackendProof {
    pub bytes: usize,
    pub public_inputs: Vec<F>,
}

#[derive(Clone, Debug)]
pub struct ProofStats {
    pub witness_ms: f64,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct BackendStatus {
    pub status: String,
    pub note: String,
}

pub trait ProofBackend {
    type Witness;

    fn prove(&self, witness: Self::Witness, public_inputs: &[F]) -> Result<BackendProof>;
    fn verify(&self, proof: &BackendProof, public_inputs: &[F]) -> Result<()>;
    fn proof_bytes(&self, proof: &BackendProof) -> usize {
        proof.bytes
    }
    fn public_inputs<'a>(&self, proof: &'a BackendProof) -> &'a [F] {
        &proof.public_inputs
    }
    fn status(&self) -> BackendStatus;
}
