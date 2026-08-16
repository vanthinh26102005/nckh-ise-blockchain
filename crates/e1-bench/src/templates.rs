use crate::backend::ProofStats;
use crate::epcis::{EpcisEventV1, PointE6, SimplePolygon, MAX_POLYGON_VERTICES};
use crate::synthetic::{poseidon2_permute, tampered_actor_lot, SyntheticLot};
use crate::types::{
    f, signed_f, CircuitKind, Profile, F, POSEIDON_TAG_CERT, POSEIDON_TAG_EMPTY,
    POSEIDON_TAG_EVENT_BATCH, POSEIDON_TAG_NULLIFIER, POSEIDON_TAG_NULLIFIER_INDEX,
    POSEIDON_TAG_POLYGON, RANGE_BITS, STRICT_MERKLE_DEPTH, STRICT_MERKLE_LEAVES, THRESHOLD,
};
use anyhow::{anyhow, bail, Result};
use core::borrow::Borrow;
use p3_air::{Air, AirBuilder, BaseAir, WindowAccess};
use p3_challenger::DuplexChallenger;
#[cfg(feature = "recursion")]
use p3_circuit::ops::{
    generate_poseidon2_trace, generate_recompose_trace, GoldilocksD2Width8, Poseidon2Config,
};
#[cfg(feature = "recursion")]
use p3_circuit_prover::{BatchStarkProver, ConstraintProfile, TablePacking};
use p3_commit::ExtensionMmcs;
#[cfg(feature = "recursion")]
use p3_commit::Pcs as PcsTrait;
use p3_dft::Radix2DitParallel;
use p3_field::extension::BinomialExtensionField;
use p3_field::{Dup, Field, PrimeCharacteristicRing, PrimeField64};
use p3_fri::{FriParameters, TwoAdicFriPcs};
use p3_goldilocks::{
    GenericPoseidon2LinearLayersGoldilocks, Poseidon2Goldilocks,
    GOLDILOCKS_POSEIDON2_HALF_FULL_ROUNDS, GOLDILOCKS_POSEIDON2_PARTIAL_ROUNDS_8,
    GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_FINAL, GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_INITIAL,
    GOLDILOCKS_POSEIDON2_RC_8_INTERNAL,
};
#[cfg(feature = "recursion")]
use p3_lookup::logup::LogUpGadget;
use p3_matrix::dense::RowMajorMatrix;
use p3_merkle_tree::MerkleTreeMmcs;
use p3_poseidon2::GenericPoseidon2LinearLayers;
use p3_poseidon2_air::{
    generate_trace_rows, FullRound, PartialRound, Poseidon2Air, Poseidon2Cols, RoundConstants, SBox,
};
#[cfg(feature = "recursion")]
use p3_recursion::pcs::{
    set_fri_mmcs_private_data, FriProofTargets, InputProofTargets, MerkleCapTargets,
    RecExtensionValMmcs, RecValMmcs, Witness,
};
#[cfg(feature = "recursion")]
use p3_recursion::traits::{RecursiveAir, RecursivePcs};
#[cfg(feature = "recursion")]
use p3_recursion::{
    build_and_prove_aggregation_layer, BatchOnly, FriRecursionBackend, FriRecursionConfig,
    FriVerifierParams, ProveNextLayerParams, RecursionInput, VerificationError,
};
use p3_symmetric::{PaddingFreeSponge, TruncatedPermutation};
use p3_uni_stark::{
    prove, verify, Proof, ProverConstraintFolder, StarkConfig, SymbolicAirBuilder,
    VerifierConstraintFolder,
};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(feature = "recursion")]
use std::sync::Arc;
use std::time::Instant;

const POSEIDON_WIDTH: usize = 8;
const GOLDILOCKS_SBOX_DEGREE: u64 = 7;
const SBOX_REGISTERS: usize = 1;
const FRI_LOG_BLOWUP: usize = 2;
const HALF_FULL_ROUNDS: usize = GOLDILOCKS_POSEIDON2_HALF_FULL_ROUNDS;
const PARTIAL_ROUNDS: usize = GOLDILOCKS_POSEIDON2_PARTIAL_ROUNDS_8;
const POSEIDON_COLS: usize = p3_poseidon2_air::num_cols::<
    POSEIDON_WIDTH,
    GOLDILOCKS_SBOX_DEGREE,
    SBOX_REGISTERS,
    HALF_FULL_ROUNDS,
    PARTIAL_ROUNDS,
>();
const C2_VECTOR_LANES: usize = 32;
const C2_INDEX_BITS_START: usize = POSEIDON_COLS * C2_VECTOR_LANES;
const C2_WIDTH: usize = C2_INDEX_BITS_START + STRICT_MERKLE_DEPTH;
const C5_SPARSE_DEPTH: usize = 32;
const C5_VECTOR_LANES: usize = 128;
const C5_INDEX_BITS_START: usize = POSEIDON_COLS * C5_VECTOR_LANES;
const C5_QUOTIENT_BITS_START: usize = C5_INDEX_BITS_START + C5_SPARSE_DEPTH;
const C5_SIBLINGS_START: usize = C5_QUOTIENT_BITS_START + C5_SPARSE_DEPTH;
const C5_WIDTH: usize = C5_SIBLINGS_START + C5_SPARSE_DEPTH;
const C3_READING_COL: usize = 0;
const C3_TIMESTAMP_COL: usize = 1;
const C3_THRESHOLD_DIFF_COL: usize = 2;
const C3_TIME_DELTA_MINUS_ONE_COL: usize = 3;
const C3_IS_REAL_COL: usize = 4;
const C3_TRANSITION_ENABLED_COL: usize = 5;
const C3_REAL_COUNT_COL: usize = 6;
const C3_READING_BITS_START: usize = 7;
const C3_DIFF_BITS_START: usize = C3_READING_BITS_START + RANGE_BITS;
const C3_DELTA_BITS_START: usize = C3_DIFF_BITS_START + RANGE_BITS;
const C3_WIDTH: usize = C3_DELTA_BITS_START + RANGE_BITS;
const C3_MAX_RANGE_VALUE: u64 = (1u64 << RANGE_BITS) - 1;
const C1_COMPARE_BITS: usize = 29;
const C1_ORIENT_BITS: usize = 56;

const S_AUX: usize = POSEIDON_COLS;
const S_PX: usize = S_AUX;
const S_PY: usize = S_PX + 1;
const S_AX: usize = S_PY + 1;
const S_AY: usize = S_AX + 1;
const S_BX: usize = S_AY + 1;
const S_BY: usize = S_BX + 1;
const S_FX: usize = S_BY + 1;
const S_FY: usize = S_FX + 1;
const S_PHASE: usize = S_FY + 1;
const S_EDGE: usize = S_PHASE + 1;
const S_EDGE_ZERO: usize = S_EDGE + 1;
const S_EDGE_INV: usize = S_EDGE_ZERO + 1;
const S_LAST: usize = S_EDGE_INV + 1;
const S_REAL: usize = S_LAST + 1;
const S_REMAINING: usize = S_REAL + 1;
const S_BATCH_STATE: usize = S_REMAINING + 1;
const S_POLY_STATE: usize = S_BATCH_STATE + 1;
const S_OUTPUT: usize = S_POLY_STATE + 1;
const S_PARITY: usize = S_OUTPUT + 1;
const S_CROSS: usize = S_PARITY + 1;
const S_AY_GT: usize = S_CROSS + 1;
const S_BY_GT: usize = S_AY_GT + 1;
const S_AX_GT: usize = S_BY_GT + 1;
const S_BX_GT: usize = S_AX_GT + 1;
const S_AY_EQ: usize = S_BX_GT + 1;
const S_BY_EQ: usize = S_AY_EQ + 1;
const S_AX_EQ: usize = S_BY_EQ + 1;
const S_BX_EQ: usize = S_AX_EQ + 1;
const S_AY_INV: usize = S_BX_EQ + 1;
const S_BY_INV: usize = S_AY_INV + 1;
const S_AX_INV: usize = S_BY_INV + 1;
const S_BX_INV: usize = S_AX_INV + 1;
const S_DY_ZERO: usize = S_BX_INV + 1;
const S_DY_INV: usize = S_DY_ZERO + 1;
const S_OMAG: usize = S_DY_INV + 1;
const S_OMAG_INV: usize = S_OMAG + 1;
const S_OZERO: usize = S_OMAG_INV + 1;
const S_OPOS: usize = S_OZERO + 1;
const S_LAT_SELECT: usize = S_OPOS + 1;
const S_LAT_INV: usize = S_LAT_SELECT + 1;
const S_LON_SELECT: usize = S_LAT_INV + 1;
const S_LON_INV: usize = S_LON_SELECT + 1;
const S_Y_CLOSED: usize = S_LON_INV + 1;
const S_X_CLOSED: usize = S_Y_CLOSED + 1;
const S_EDGE_BITS: usize = S_X_CLOSED + 1;
const S_COMPARE_START: usize = S_EDGE_BITS + 5;
const S_COMPARE_WIDTH: usize = 3 + 2 * C1_COMPARE_BITS;
const S_ORIENT_BITS: usize = S_COMPARE_START + 4 * S_COMPARE_WIDTH;
const S_WIDTH: usize = S_ORIENT_BITS + C1_ORIENT_BITS;

type Challenge = BinomialExtensionField<F, 2>;
type Perm = Poseidon2Goldilocks<POSEIDON_WIDTH>;
type MyHash = PaddingFreeSponge<Perm, POSEIDON_WIDTH, 4, 4>;
type MyCompress = TruncatedPermutation<Perm, 2, 4, POSEIDON_WIDTH>;
type ValMmcs =
    MerkleTreeMmcs<<F as Field>::Packing, <F as Field>::Packing, MyHash, MyCompress, 2, 4>;
type ChallengeMmcs = ExtensionMmcs<F, Challenge, ValMmcs>;
type Dft = Radix2DitParallel<F>;
type Pcs = TwoAdicFriPcs<F, Dft, ValMmcs, ChallengeMmcs>;
type Challenger = DuplexChallenger<F, Perm, POSEIDON_WIDTH, 4>;
type MyConfig = StarkConfig<Pcs, Challenge, Challenger>;

#[derive(Clone)]
pub struct TemplateCache {
    templates: HashMap<CircuitKind, CircuitTemplate>,
}

impl TemplateCache {
    pub fn build(events: usize, profile: Profile, circuits: &[CircuitKind]) -> Result<Self> {
        let mut templates = HashMap::new();
        for &kind in circuits {
            templates.insert(kind, build_template(kind, events, profile)?);
        }
        if circuits.contains(&CircuitKind::Wrapper) {
            for kind in [
                CircuitKind::C2,
                CircuitKind::C3,
                CircuitKind::C4,
                CircuitKind::C5,
            ] {
                templates
                    .entry(kind)
                    .or_insert(build_template(kind, events, profile)?);
            }
        }
        Ok(Self { templates })
    }

    pub fn get(&self, kind: CircuitKind) -> &CircuitTemplate {
        self.templates
            .get(&kind)
            .unwrap_or_else(|| panic!("missing template for {kind:?}"))
    }
}

#[derive(Clone)]
pub struct CircuitTemplate {
    pub kind: CircuitKind,
    pub build_ms: f64,
    pub gate_count: usize,
    pub public_inputs: usize,
    pub note: &'static str,
    events: usize,
    profile: Profile,
}

pub struct InnerProofBundle {
    pub proof_bytes: usize,
    pub prove_ms: f64,
}

pub struct WitnessBundle {
    pub witness_ms: f64,
}

#[derive(Debug)]
pub struct RecursiveProofStats {
    pub witness_ms: f64,
    pub prove_ms: f64,
    pub verify_ms: f64,
    pub proof_bytes: usize,
    pub inner_prove_ms: f64,
}

struct StarkProofBundle<SC: p3_uni_stark::StarkGenericConfig> {
    proof: Proof<SC>,
    prove_ms: f64,
    verify_ms: f64,
    proof_bytes: usize,
}

pub fn build_template(
    kind: CircuitKind,
    events: usize,
    profile: Profile,
) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let (public_inputs, note) = match kind {
        CircuitKind::C1 => (
            2,
            "research;real:c1 proves every private WGS-84 event point is strictly outside a private simple polygon, bound by Poseidon2 commitments",
        ),
        CircuitKind::C2 => (
            1,
            "research;real:c2 verifies a private certificate/event membership path against a public Poseidon2 Merkle root",
        ),
        CircuitKind::C3 => (
            2,
            "research;real:c3 proves threshold/time baseline in a Plonky3 STARK with 32-bit range and strict timestamp AIR constraints",
        ),
        CircuitKind::C4 => (
            3,
            "research;real:c4 proves ZK-native Poseidon2 actor authorization; Ed25519/EdDSA remains a separate blocker",
        ),
        CircuitKind::C5 => (
            4,
            "research;real:c5 proves a 32-bit sparse Poseidon2 nullifier-map update from public old root to public new root; host persistence is deferred to E2/Fabric",
        ),
        CircuitKind::Wrapper => (
            0,
            "blocked;recursion:Plonky3-recursion GitHub rev 524665d is pinned but currently panics in aggregation; no mocked recursive proof emitted",
        ),
        CircuitKind::C1Legacy => (
            0,
            "compat;disabled:legacy placeholder circuit removed from the Plonky3 backend",
        ),
    };
    Ok(CircuitTemplate {
        kind,
        build_ms: start.elapsed().as_secs_f64() * 1000.0,
        gate_count: 0,
        public_inputs,
        note,
        events,
        profile,
    })
}

pub fn prove_and_verify(
    template: &CircuitTemplate,
    lot: &SyntheticLot,
    seed: usize,
    profile: Profile,
) -> Result<ProofStats> {
    if profile != template.profile {
        bail!("template profile mismatch");
    }
    match template.kind {
        CircuitKind::C1 => prove_c1(lot, template.events),
        CircuitKind::C2 => prove_c2(lot, seed, template.events),
        CircuitKind::C3 => prove_c3(lot, template.events),
        CircuitKind::C4 => prove_c4(lot),
        CircuitKind::C5 => prove_c5(lot),
        CircuitKind::Wrapper => bail!(
            "Plonky3 recursion integration is only available through the wrapper runner with --features recursion"
        ),
        CircuitKind::C1Legacy => {
            bail!(
                "{} is disabled in the Plonky3 base backend",
                template.kind.as_str()
            )
        }
    }
}

pub fn prove_inner(
    template: &CircuitTemplate,
    lot: &SyntheticLot,
    seed: usize,
    profile: Profile,
) -> Result<InnerProofBundle> {
    let stats = prove_and_verify(template, lot, seed, profile)?;
    Ok(InnerProofBundle {
        proof_bytes: stats.proof_bytes,
        prove_ms: stats.prove_ms,
    })
}

pub fn witness_for(
    template: &CircuitTemplate,
    lot: &SyntheticLot,
    seed: usize,
    profile: Profile,
) -> Result<WitnessBundle> {
    let start = Instant::now();
    match template.kind {
        CircuitKind::C1 => {
            let _ = c1_serial_trace_and_pis(lot, template.events)?;
        }
        CircuitKind::C2 => {
            let _ = c2_trace_and_pis(lot, seed, template.events, None)?;
        }
        CircuitKind::C3 => validate_c3(lot, template.events)?,
        CircuitKind::C4 => {
            let _ = c4_trace_and_pis(lot, None)?;
        }
        CircuitKind::C5 => {
            let _ = c5_trace_and_pis(lot, None)?;
        }
        CircuitKind::Wrapper => bail!("recursive witness is blocked"),
        CircuitKind::C1Legacy => {
            bail!("disabled circuit")
        }
    }
    let _ = (seed, profile);
    Ok(WitnessBundle {
        witness_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

pub fn wrapper_witness(
    _wrapper: &CircuitTemplate,
    _inner_proofs: &[usize],
) -> Result<WitnessBundle> {
    bail!(
        "Plonky3 recursive wrapper requires --features recursion and is currently blocked by upstream aggregation panic at pinned rev 524665d"
    )
}

fn config() -> MyConfig {
    let perm = poseidon_perm();
    let hash = MyHash::new(perm.clone());
    let compress = MyCompress::new(perm.clone());
    let val_mmcs = ValMmcs::new(hash, compress, 0);
    let challenge_mmcs = ChallengeMmcs::new(val_mmcs.clone());
    let dft = Dft::default();
    let fri_params = FriParameters {
        log_blowup: FRI_LOG_BLOWUP,
        log_final_poly_len: 0,
        max_log_arity: 1,
        num_queries: 8,
        commit_proof_of_work_bits: 0,
        query_proof_of_work_bits: 0,
        mmcs: challenge_mmcs,
    };
    let pcs = Pcs::new(dft, val_mmcs, fri_params);
    let challenger = Challenger::new(perm);
    MyConfig::new(pcs, challenger)
}

#[cfg(feature = "recursion")]
type RecInputProof = InputProofTargets<F, Challenge, RecValMmcs<F, 4, MyHash, MyCompress>>;
#[cfg(feature = "recursion")]
type RecOpeningProof = FriProofTargets<
    F,
    Challenge,
    RecExtensionValMmcs<F, Challenge, 4, RecValMmcs<F, 4, MyHash, MyCompress>>,
    RecInputProof,
    Witness<F>,
>;

#[cfg(feature = "recursion")]
#[derive(Clone)]
struct RecursionConfig {
    config: Arc<MyConfig>,
    fri_verifier_params: FriVerifierParams,
}

#[cfg(feature = "recursion")]
impl core::ops::Deref for RecursionConfig {
    type Target = MyConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

#[cfg(feature = "recursion")]
impl p3_uni_stark::StarkGenericConfig for RecursionConfig {
    type Challenge = Challenge;
    type Challenger = Challenger;
    type Pcs = Pcs;

    fn pcs(&self) -> &Self::Pcs {
        self.config.pcs()
    }

    fn initialise_challenger(&self) -> Self::Challenger {
        self.config.initialise_challenger()
    }
}

#[cfg(feature = "recursion")]
impl FriRecursionConfig for RecursionConfig
where
    Pcs: RecursivePcs<
        RecursionConfig,
        RecInputProof,
        RecOpeningProof,
        MerkleCapTargets<F, 4>,
        <Pcs as PcsTrait<Challenge, Challenger>>::Domain,
    >,
{
    type Commitment = MerkleCapTargets<F, 4>;
    type InputProof = RecInputProof;
    type OpeningProof = RecOpeningProof;
    type RawOpeningProof = <Pcs as PcsTrait<Challenge, Challenger>>::Proof;
    const DIGEST_ELEMS: usize = 4;

    fn with_fri_opening_proof<'a, A, R>(
        prev: &RecursionInput<'a, Self, A>,
        f: impl FnOnce(&Self::RawOpeningProof) -> R,
    ) -> R
    where
        A: RecursiveAir<F, Challenge, LogUpGadget>,
    {
        match prev {
            RecursionInput::UniStark { proof, .. } => f(&proof.opening_proof),
            RecursionInput::BatchStark { proof, .. } => f(&proof.proof.opening_proof),
        }
    }

    fn prepare_circuit_for_verification(
        &self,
        circuit: &mut p3_circuit::CircuitBuilder<Challenge>,
    ) -> Result<(), VerificationError> {
        circuit.enable_poseidon2_perm_width_8::<GoldilocksD2Width8, _>(
            generate_poseidon2_trace::<Challenge, GoldilocksD2Width8>,
            poseidon_perm(),
        );
        circuit.enable_recompose::<F>(generate_recompose_trace::<F, Challenge>);
        Ok(())
    }

    fn pcs_verifier_params(
        &self,
    ) -> &<Pcs as RecursivePcs<
        RecursionConfig,
        RecInputProof,
        RecOpeningProof,
        MerkleCapTargets<F, 4>,
        <Pcs as PcsTrait<Challenge, Challenger>>::Domain,
    >>::VerifierParams {
        &self.fri_verifier_params
    }

    fn set_fri_private_data(
        runner: &mut p3_circuit::CircuitRunner<'_, Challenge>,
        op_ids: &[p3_circuit::NonPrimitiveOpId],
        opening_proof: &Self::RawOpeningProof,
    ) -> Result<(), &'static str> {
        set_fri_mmcs_private_data::<F, Challenge, ChallengeMmcs, ValMmcs, MyHash, MyCompress, 4>(
            runner,
            op_ids,
            opening_proof,
            Poseidon2Config::GOLDILOCKS_D2_W8,
        )
    }
}

#[cfg(feature = "recursion")]
fn recursion_config() -> RecursionConfig {
    RecursionConfig {
        config: Arc::new(config()),
        fri_verifier_params: FriVerifierParams::with_mmcs(
            2,
            0,
            0,
            0,
            Poseidon2Config::GOLDILOCKS_D2_W8,
        ),
    }
}

fn poseidon_perm() -> Perm {
    let external = p3_poseidon2::ExternalLayerConstants::new(
        GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_INITIAL.to_vec(),
        GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_FINAL.to_vec(),
    );
    Poseidon2Goldilocks::<POSEIDON_WIDTH>::new(&external, &GOLDILOCKS_POSEIDON2_RC_8_INTERNAL)
}

fn poseidon_constants() -> RoundConstants<F, POSEIDON_WIDTH, HALF_FULL_ROUNDS, PARTIAL_ROUNDS> {
    RoundConstants::new(
        GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_INITIAL,
        GOLDILOCKS_POSEIDON2_RC_8_INTERNAL,
        GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_FINAL,
    )
}

fn prove_stark_artifact<A>(
    air: &A,
    trace: RowMajorMatrix<F>,
    pis: &[F],
) -> Result<StarkProofBundle<MyConfig>>
where
    A: BaseAir<F>
        + Air<SymbolicAirBuilder<F>>
        + for<'a> Air<ProverConstraintFolder<'a, MyConfig>>
        + for<'a> Air<VerifierConstraintFolder<'a, MyConfig>>
        + for<'a> Air<p3_air::DebugConstraintBuilder<'a, F>>,
{
    let cfg = config();
    let prove_start = Instant::now();
    let proof = catch_unwind(AssertUnwindSafe(|| prove(&cfg, air, trace, pis)))
        .map_err(|_| anyhow!("prove failed: constraints were not satisfied"))?;
    let prove_ms = prove_start.elapsed().as_secs_f64() * 1000.0;
    let bytes = postcard::to_allocvec(&proof)?.len();
    let verify_start = Instant::now();
    catch_unwind(AssertUnwindSafe(|| verify(&cfg, air, &proof, pis)))
        .map_err(|_| anyhow!("verify panicked"))?
        .map_err(|err| anyhow!("verify failed: {err:?}"))?;
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
    Ok(StarkProofBundle {
        proof,
        prove_ms,
        verify_ms,
        proof_bytes: bytes,
    })
}

fn prove_stark<A>(air: &A, trace: RowMajorMatrix<F>, pis: &[F]) -> Result<(f64, f64, usize)>
where
    A: BaseAir<F>
        + Air<SymbolicAirBuilder<F>>
        + for<'a> Air<ProverConstraintFolder<'a, MyConfig>>
        + for<'a> Air<VerifierConstraintFolder<'a, MyConfig>>
        + for<'a> Air<p3_air::DebugConstraintBuilder<'a, F>>,
{
    let bundle = prove_stark_artifact(air, trace, pis)?;
    let _ = bundle.proof;
    Ok((bundle.prove_ms, bundle.verify_ms, bundle.proof_bytes))
}

#[cfg(feature = "recursion")]
fn prove_recursion_stark<A>(
    cfg: &RecursionConfig,
    air: &A,
    trace: RowMajorMatrix<F>,
    pis: &[F],
) -> Result<StarkProofBundle<RecursionConfig>>
where
    A: BaseAir<F>
        + Air<SymbolicAirBuilder<F>>
        + for<'a> Air<ProverConstraintFolder<'a, RecursionConfig>>
        + for<'a> Air<VerifierConstraintFolder<'a, RecursionConfig>>
        + for<'a> Air<p3_air::DebugConstraintBuilder<'a, F>>,
{
    let prove_start = Instant::now();
    let proof = catch_unwind(AssertUnwindSafe(|| prove(cfg, air, trace, pis)))
        .map_err(|_| anyhow!("prove failed: constraints were not satisfied"))?;
    let prove_ms = prove_start.elapsed().as_secs_f64() * 1000.0;
    let proof_bytes = postcard::to_allocvec(&proof)?.len();
    let verify_start = Instant::now();
    catch_unwind(AssertUnwindSafe(|| verify(cfg, air, &proof, pis)))
        .map_err(|_| anyhow!("verify panicked"))?
        .map_err(|err| anyhow!("verify failed: {err:?}"))?;
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
    Ok(StarkProofBundle {
        proof,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

#[cfg(feature = "recursion")]
fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[cfg(feature = "recursion")]
fn recursion_step<T>(
    label: &str,
    step: impl FnOnce() -> Result<T, VerificationError>,
) -> Result<T> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(step));
    std::panic::set_hook(previous_hook);

    result
        .map_err(|payload| {
            anyhow!(
                "upstream Plonky3-recursion panicked during {label}: {}",
                panic_payload_message(payload)
            )
        })?
        .map_err(|err| anyhow!("upstream Plonky3-recursion failed during {label}: {err:?}"))
}

#[cfg(feature = "recursion")]
pub fn prove_recursive_wrapper(
    lot: &SyntheticLot,
    seed: usize,
    events: usize,
    _profile: Profile,
) -> Result<RecursiveProofStats> {
    let witness_start = Instant::now();
    let (c2_trace, c2_pis) = c2_trace_and_pis(lot, seed, events, None)?;
    validate_c3(lot, events)?;
    let c3_trace = c3_trace(lot, events)?;
    let c3_pis = vec![f(THRESHOLD), f(events as u64)];
    let (c4_trace, c4_pis) = c4_trace_and_pis(lot, None)?;
    let (c5_trace, c5_pis) = c5_trace_and_pis(lot, None)?;
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;

    let cfg = recursion_config();
    let c2_air = C2MerkleAir;
    let c3_air = C3Air;
    let c4_air = PoseidonStatementAir {
        public_input_indices: [Some(0), None, Some(2), None],
        num_pis: 3,
    };
    let c5_air = C5NullifierUpdateAir;

    let c2 = prove_recursion_stark(&cfg, &c2_air, c2_trace, &c2_pis)?;
    let c3 = prove_recursion_stark(&cfg, &c3_air, c3_trace, &c3_pis)?;
    let c4 = prove_recursion_stark(&cfg, &c4_air, c4_trace, &c4_pis)?;
    let c5 = prove_recursion_stark(&cfg, &c5_air, c5_trace, &c5_pis)?;
    let inner_prove_ms = c2.prove_ms + c3.prove_ms + c4.prove_ms + c5.prove_ms;

    let backend =
        FriRecursionBackend::<POSEIDON_WIDTH, 4, _>::new(Poseidon2Config::GOLDILOCKS_D2_W8)
            .for_extension_degree::<2>();
    let params = ProveNextLayerParams {
        table_packing: TablePacking::new(1, 2).with_fri_params(0, 2),
        constraint_profile: ConstraintProfile::Standard,
    };

    let recursive_start = Instant::now();
    let c2_input = RecursionInput::UniStark {
        proof: &c2.proof,
        air: &c2_air,
        public_inputs: c2_pis,
        preprocessed_commit: None,
    };
    let c3_input = RecursionInput::UniStark {
        proof: &c3.proof,
        air: &c3_air,
        public_inputs: c3_pis,
        preprocessed_commit: None,
    };
    let c4_input = RecursionInput::UniStark {
        proof: &c4.proof,
        air: &c4_air,
        public_inputs: c4_pis,
        preprocessed_commit: None,
    };
    let c5_input = RecursionInput::UniStark {
        proof: &c5.proof,
        air: &c5_air,
        public_inputs: c5_pis,
        preprocessed_commit: None,
    };

    let left_layer = recursion_step("aggregation C2+C3", || {
        build_and_prove_aggregation_layer::<RecursionConfig, _, _, _, 2>(
            &c2_input, &c3_input, &cfg, &backend, &params, None,
        )
    })?;
    let right_layer = recursion_step("aggregation C4+C5", || {
        build_and_prove_aggregation_layer::<RecursionConfig, _, _, _, 2>(
            &c4_input, &c5_input, &cfg, &backend, &params, None,
        )
    })?;
    let left_input = left_layer.into_recursion_input::<BatchOnly>();
    let right_input = right_layer.into_recursion_input::<BatchOnly>();
    let final_layer = recursion_step("final aggregation", || {
        build_and_prove_aggregation_layer::<RecursionConfig, _, _, _, 2>(
            &left_input,
            &right_input,
            &cfg,
            &backend,
            &params,
            None,
        )
    })?;
    let prove_ms = recursive_start.elapsed().as_secs_f64() * 1000.0;
    let proof_bytes = postcard::to_allocvec(&final_layer.0)?.len();

    let verify_start = Instant::now();
    let mut verifier =
        BatchStarkProver::new(cfg.clone()).with_table_packing(params.table_packing.clone());
    verifier.register_poseidon2_table::<2>(Poseidon2Config::GOLDILOCKS_D2_W8);
    verifier.register_recompose_table::<2>(false);
    catch_unwind(AssertUnwindSafe(|| {
        verifier.verify_all_tables(&final_layer.0)
    }))
    .map_err(|payload| {
        anyhow!(
            "upstream Plonky3-recursion panicked during final verify: {}",
            panic_payload_message(payload)
        )
    })?
    .map_err(|err| anyhow!("recursive final proof verify failed: {err:?}"))?;
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    Ok(RecursiveProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
        inner_prove_ms,
    })
}

fn prove_c1(lot: &SyntheticLot, events: usize) -> Result<ProofStats> {
    let witness_start = Instant::now();
    let (trace, pis) = c1_serial_trace_and_pis(lot, events)?;
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(
        &C1SerialAir {
            events,
            level: usize::MAX,
        },
        trace,
        &pis,
    )?;
    Ok(ProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn c1_padded_vertices(polygon: &SimplePolygon) -> Vec<PointE6> {
    let mut vertices = polygon.vertices().to_vec();
    let last = *vertices
        .last()
        .expect("SimplePolygon validates its minimum vertex count");
    vertices.resize(MAX_POLYGON_VERTICES, last);
    vertices
}

fn c1_polygon_input(acc: F, vertex: PointE6) -> [F; POSEIDON_WIDTH] {
    [
        acc,
        f((vertex.longitude_e6 as i64 + 180_000_000) as u64),
        f((vertex.latitude_e6 as i64 + 90_000_000) as u64),
        f(POSEIDON_TAG_POLYGON),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(4),
    ]
}

fn c1_polygon_root(vertices: &[PointE6]) -> F {
    vertices
        .iter()
        .fold(f(POSEIDON_TAG_POLYGON), |acc, vertex| {
            poseidon2_permute(c1_polygon_input(acc, *vertex))[0]
        })
}

fn c1_closed_intervals(point: PointE6, start: PointE6, end: PointE6) -> (F, F) {
    let y_straddle =
        (start.latitude_e6 > point.latitude_e6) != (end.latitude_e6 > point.latitude_e6);
    let y_closed = y_straddle
        || start.latitude_e6 == point.latitude_e6
        || end.latitude_e6 == point.latitude_e6;
    let x_straddle =
        (start.longitude_e6 > point.longitude_e6) != (end.longitude_e6 > point.longitude_e6);
    let x_closed = x_straddle
        || start.longitude_e6 == point.longitude_e6
        || end.longitude_e6 == point.longitude_e6;
    (
        if y_closed { F::ONE } else { F::ZERO },
        if x_closed { F::ONE } else { F::ZERO },
    )
}

fn c1_serial_trace_and_pis(
    lot: &SyntheticLot,
    events: usize,
) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    if !(8..=64).contains(&events) || lot.events.len() != events {
        bail!("c1 requires exactly 8 to 64 EPCIS events");
    }
    for event in &lot.events {
        event.validate()?;
    }
    let vertices = c1_padded_vertices(&lot.polygon);
    let polygon_root = c1_polygon_root(&vertices);
    let height = events.next_power_of_two() * MAX_POLYGON_VERTICES * 2;
    let real_edges = events * MAX_POLYGON_VERTICES;
    let mut inputs = Vec::with_capacity(height);
    let mut aux = vec![F::ZERO; height * (S_WIDTH - S_AUX)];
    let mut batch_state = f(POSEIDON_TAG_EVENT_BATCH);
    let mut poly_state = f(POSEIDON_TAG_POLYGON);
    let mut parity = F::ZERO;

    for row in 0..height {
        let phase = row & 1;
        let pair = row / 2;
        let edge = pair % MAX_POLYGON_VERTICES;
        let real = pair < real_edges;
        let event = &lot.events[(pair / MAX_POLYGON_VERTICES).min(events - 1)];
        let point = event.point();
        let start = vertices[edge];
        let end = vertices[(edge + 1) % MAX_POLYGON_VERTICES];
        let batch_before = batch_state;
        let poly_before = poly_state;
        let (input, output, cross, parity_before) = if phase == 0 {
            let word = if real && edge < 23 {
                c1_event_words(event)[edge]
            } else {
                F::ZERO
            };
            let input = [
                batch_state,
                word,
                f(POSEIDON_TAG_EVENT_BATCH),
                F::ZERO,
                F::ZERO,
                F::ZERO,
                F::ZERO,
                f(3),
            ];
            let output = poseidon2_permute(input)[0];
            batch_state = output;
            let before = parity;
            if c1_crosses_right(point, start, end) {
                parity = F::ONE - parity;
            }
            (input, output, c1_crosses_right(point, start, end), before)
        } else {
            let input = c1_polygon_input(poly_state, start);
            let output = poseidon2_permute(input)[0];
            if edge + 1 == MAX_POLYGON_VERTICES {
                poly_state = f(POSEIDON_TAG_POLYGON);
            } else {
                poly_state = output;
            }
            let before = parity;
            if edge + 1 == MAX_POLYGON_VERTICES {
                parity = F::ZERO;
            }
            (input, output, false, before)
        };
        inputs.push(input);
        let base = row * (S_WIDTH - S_AUX);
        s_fill_aux(
            &mut aux[base..base + (S_WIDTH - S_AUX)],
            point,
            start,
            end,
            vertices[0],
            phase,
            edge,
            real,
            real_edges - pair.min(real_edges),
            batch_before,
            poly_before,
            output,
            parity_before,
            cross,
        );
    }

    let poseidon_rows = inputs
        .into_iter()
        .flat_map(|input| poseidon_trace(vec![input]).values)
        .collect::<Vec<_>>();
    let mut values = Vec::with_capacity(height * S_WIDTH);
    for row in 0..height {
        values.extend_from_slice(&poseidon_rows[row * POSEIDON_COLS..(row + 1) * POSEIDON_COLS]);
        let base = row * (S_WIDTH - S_AUX);
        values.extend_from_slice(&aux[base..base + (S_WIDTH - S_AUX)]);
    }
    Ok((
        RowMajorMatrix::new(values, S_WIDTH),
        vec![polygon_root, batch_state],
    ))
}

fn c1_event_words(event: &EpcisEventV1) -> [F; 23] {
    let hi = |value: u64| f(value >> 32);
    let lo = |value: u64| f(value & u32::MAX as u64);
    let mut words = [F::ZERO; 23];
    words[..15].copy_from_slice(&[
        f(1),
        hi(event.event_id),
        lo(event.event_id),
        hi(event.lot_id),
        lo(event.lot_id),
        hi(event.epoch_id),
        lo(event.epoch_id),
        hi(event.timestamp_ms),
        lo(event.timestamp_ms),
        f(event.readings as u64),
        f((event.latitude_e6 as i64 + 90_000_000) as u64),
        f((event.longitude_e6 as i64 + 180_000_000) as u64),
        hi(event.certificate_id),
        lo(event.certificate_id),
        f(event.role as u64),
    ]);
    for (index, chunk) in event.actor_public_key.chunks_exact(4).enumerate() {
        words[15 + index] =
            f(u32::from_be_bytes(chunk.try_into().expect("4-byte key chunk")) as u64);
    }
    words
}

fn c1_crosses_right(point: PointE6, start: PointE6, end: PointE6) -> bool {
    let straddles =
        (start.latitude_e6 > point.latitude_e6) != (end.latitude_e6 > point.latitude_e6);
    let orientation = (end.longitude_e6 as i128 - start.longitude_e6 as i128)
        * (point.latitude_e6 as i128 - start.latitude_e6 as i128)
        - (end.latitude_e6 as i128 - start.latitude_e6 as i128)
            * (point.longitude_e6 as i128 - start.longitude_e6 as i128);
    straddles && ((orientation > 0) == (end.latitude_e6 > point.latitude_e6))
}

#[allow(clippy::too_many_arguments)]
fn s_fill_aux(
    values: &mut [F],
    point: PointE6,
    start: PointE6,
    end: PointE6,
    first: PointE6,
    phase: usize,
    edge: usize,
    real: bool,
    remaining: usize,
    batch_state: F,
    poly_state: F,
    output: F,
    parity: F,
    cross: bool,
) {
    let set = |values: &mut [F], column: usize, value: F| values[column - S_AUX] = value;
    set(values, S_PX, signed_f(point.longitude_e6 as i64));
    set(values, S_PY, signed_f(point.latitude_e6 as i64));
    set(values, S_AX, signed_f(start.longitude_e6 as i64));
    set(values, S_AY, signed_f(start.latitude_e6 as i64));
    set(values, S_BX, signed_f(end.longitude_e6 as i64));
    set(values, S_BY, signed_f(end.latitude_e6 as i64));
    set(values, S_FX, signed_f(first.longitude_e6 as i64));
    set(values, S_FY, signed_f(first.latitude_e6 as i64));
    set(values, S_PHASE, f(phase as u64));
    set(values, S_EDGE, f(edge as u64));
    set(
        values,
        S_EDGE_ZERO,
        if edge == 0 { F::ONE } else { F::ZERO },
    );
    set(
        values,
        S_EDGE_INV,
        if edge == 0 {
            F::ZERO
        } else {
            f(edge as u64).inverse()
        },
    );
    set(
        values,
        S_LAST,
        if edge + 1 == MAX_POLYGON_VERTICES {
            F::ONE
        } else {
            F::ZERO
        },
    );
    set(values, S_REAL, if real { F::ONE } else { F::ZERO });
    set(values, S_REMAINING, f(remaining as u64));
    set(values, S_BATCH_STATE, batch_state);
    set(values, S_POLY_STATE, poly_state);
    set(values, S_OUTPUT, output);
    set(values, S_PARITY, parity);
    set(values, S_CROSS, if cross { F::ONE } else { F::ZERO });

    let px = point.longitude_e6 as i64;
    let py = point.latitude_e6 as i64;
    let ax = start.longitude_e6 as i64;
    let ay = start.latitude_e6 as i64;
    let bx = end.longitude_e6 as i64;
    let by = end.latitude_e6 as i64;
    for (index, (left, right)) in [(ay, py), (by, py), (ax, px), (bx, px)]
        .into_iter()
        .enumerate()
    {
        s_fill_comparison(values, index, left, right);
        let equal_col = [S_AY_EQ, S_BY_EQ, S_AX_EQ, S_BX_EQ][index];
        let inverse_col = [S_AY_INV, S_BY_INV, S_AX_INV, S_BX_INV][index];
        let difference = left - right;
        set(
            values,
            equal_col,
            if difference == 0 { F::ONE } else { F::ZERO },
        );
        set(
            values,
            inverse_col,
            if difference == 0 {
                F::ZERO
            } else {
                signed_f(difference).inverse()
            },
        );
    }
    let dy = by - ay;
    set(values, S_DY_ZERO, if dy == 0 { F::ONE } else { F::ZERO });
    set(
        values,
        S_DY_INV,
        if dy == 0 {
            F::ZERO
        } else {
            signed_f(dy).inverse()
        },
    );
    let orient = (bx - ax) as i128 * (py - ay) as i128 - (by - ay) as i128 * (px - ax) as i128;
    let mag = orient.unsigned_abs() as u64;
    set(values, S_OMAG, f(mag));
    set(
        values,
        S_OMAG_INV,
        if mag == 0 { F::ZERO } else { f(mag).inverse() },
    );
    set(values, S_OZERO, if mag == 0 { F::ONE } else { F::ZERO });
    set(values, S_OPOS, if orient > 0 { F::ONE } else { F::ZERO });
    s_fill_edge_equal(values, S_LAT_SELECT, S_LAT_INV, edge, 10);
    s_fill_edge_equal(values, S_LON_SELECT, S_LON_INV, edge, 11);
    let (y_closed, x_closed) = c1_closed_intervals(point, start, end);
    set(values, S_Y_CLOSED, y_closed);
    set(values, S_X_CLOSED, x_closed);
    s_fill_bits(values, S_EDGE_BITS, edge as u64, 5);
    s_fill_bits(values, S_ORIENT_BITS, mag, C1_ORIENT_BITS);
}

fn s_fill_edge_equal(values: &mut [F], select: usize, inverse: usize, edge: usize, target: usize) {
    let difference = edge as i64 - target as i64;
    values[select - S_AUX] = if difference == 0 { F::ONE } else { F::ZERO };
    values[inverse - S_AUX] = if difference == 0 {
        F::ZERO
    } else {
        signed_f(difference).inverse()
    };
}

fn s_fill_comparison(values: &mut [F], index: usize, left: i64, right: i64) {
    let start = S_COMPARE_START + index * S_COMPARE_WIDTH;
    let (greater, greater_diff, other_diff) = if left > right {
        (F::ONE, (left - right - 1) as u64, 0)
    } else {
        (F::ZERO, 0, (right - left) as u64)
    };
    values[start - S_AUX] = greater;
    values[start + 1 - S_AUX] = f(greater_diff);
    values[start + 2 - S_AUX] = f(other_diff);
    s_fill_bits(values, start + 3, greater_diff, C1_COMPARE_BITS);
    s_fill_bits(
        values,
        start + 3 + C1_COMPARE_BITS,
        other_diff,
        C1_COMPARE_BITS,
    );
}

fn s_fill_bits(values: &mut [F], start: usize, value: u64, bits: usize) {
    for bit in 0..bits {
        values[start + bit - S_AUX] = if value >> bit & 1 == 1 {
            F::ONE
        } else {
            F::ZERO
        };
    }
}

pub struct C1SerialAir {
    events: usize,
    level: usize,
}

impl BaseAir<F> for C1SerialAir {
    fn width(&self) -> usize {
        S_WIDTH
    }
    fn main_next_row_columns(&self) -> Vec<usize> {
        if self.level == 0 {
            vec![]
        } else {
            (0..S_WIDTH).collect()
        }
    }
    fn num_public_values(&self) -> usize {
        2
    }
    fn max_constraint_degree(&self) -> Option<usize> {
        None
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for C1SerialAir {
    fn eval(&self, builder: &mut AB) {
        let polygon_root: AB::Expr = builder.public_values()[0].into();
        let event_root: AB::Expr = builder.public_values()[1].into();
        let main = builder.main();
        let local = main.current_slice();
        let next = main.next_slice();
        let poseidon = poseidon_lane::<AB>(local, 0);
        let next_poseidon = poseidon_lane::<AB>(next, 0);
        eval_poseidon2_cols(builder, poseidon);
        if self.level == 0 {
            return;
        }
        let poseidon_out = poseidon.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];

        let px = local[S_PX];
        let py = local[S_PY];
        let ax = local[S_AX];
        let ay = local[S_AY];
        let bx = local[S_BX];
        let by = local[S_BY];
        let fx = local[S_FX];
        let fy = local[S_FY];
        let phase = local[S_PHASE];
        let edge = local[S_EDGE];
        let edge_zero = local[S_EDGE_ZERO];
        let edge_inv = local[S_EDGE_INV];
        let last = local[S_LAST];
        let real = local[S_REAL];
        let remaining = local[S_REMAINING];
        let batch = local[S_BATCH_STATE];
        let poly = local[S_POLY_STATE];
        let output = local[S_OUTPUT];
        let parity = local[S_PARITY];
        let cross = local[S_CROSS];

        for value in [phase, edge_zero, last, real, parity, cross] {
            s_bool(builder, value);
        }
        builder.assert_zero(edge_zero * edge);
        builder.assert_zero(edge * edge_inv + edge_zero - F::ONE);
        builder.assert_zero(last * (edge - f((MAX_POLYGON_VERTICES - 1) as u64)));
        s_range(builder, local, edge, S_EDGE_BITS, 5);

        builder.assert_zero(output - poseidon_out);
        builder.assert_zero(poseidon.inputs[0] - batch - phase * (poly - batch));
        builder.assert_zero((phase - F::ONE) * (poseidon.inputs[2] - f(POSEIDON_TAG_EVENT_BATCH)));
        builder.assert_zero((phase - F::ONE) * poseidon.inputs[3]);
        builder.assert_zero((phase - F::ONE) * poseidon.inputs[4]);
        builder.assert_zero((phase - F::ONE) * poseidon.inputs[5]);
        builder.assert_zero((phase - F::ONE) * poseidon.inputs[6]);
        builder.assert_zero((phase - F::ONE) * (poseidon.inputs[7] - f(3)));
        builder.assert_zero(phase * (poseidon.inputs[1] - ax - f(180_000_000)));
        builder.assert_zero(phase * (poseidon.inputs[2] - ay - f(90_000_000)));
        builder.assert_zero(phase * (poseidon.inputs[3] - f(POSEIDON_TAG_POLYGON)));
        builder.assert_zero(phase * poseidon.inputs[4]);
        builder.assert_zero(phase * poseidon.inputs[5]);
        builder.assert_zero(phase * poseidon.inputs[6]);
        builder.assert_zero(phase * (poseidon.inputs[7] - f(4)));
        builder.assert_zero((phase - F::ONE) * edge_zero * (poly - f(POSEIDON_TAG_POLYGON)));

        let ay_gt = s_compare(builder, local, 0, ay, py);
        let by_gt = s_compare(builder, local, 1, by, py);
        let ax_gt = s_compare(builder, local, 2, ax, px);
        let bx_gt = s_compare(builder, local, 3, bx, px);
        let ay_eq = s_eq(builder, ay, py, local[S_AY_EQ], local[S_AY_INV]);
        let by_eq = s_eq(builder, by, py, local[S_BY_EQ], local[S_BY_INV]);
        let ax_eq = s_eq(builder, ax, px, local[S_AX_EQ], local[S_AX_INV]);
        let bx_eq = s_eq(builder, bx, px, local[S_BX_EQ], local[S_BX_INV]);
        let dy_zero = s_eq(builder, by, ay, local[S_DY_ZERO], local[S_DY_INV]);
        let orient = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        let mag = local[S_OMAG];
        let ozero = local[S_OZERO];
        let opos = local[S_OPOS];
        s_bool(builder, ozero);
        s_bool(builder, opos);
        s_range(builder, local, mag, S_ORIENT_BITS, C1_ORIENT_BITS);
        builder.assert_zero(orient - (opos * f(2) - F::ONE) * mag);
        builder.assert_zero(mag * local[S_OMAG_INV] + ozero - F::ONE);
        let straddle = ay_gt + by_gt - ay_gt * by_gt * f(2);
        let direction = (opos - F::ONE) * (by_gt - F::ONE) + opos * by_gt;
        builder.assert_zero((phase - F::ONE) * (cross - straddle.clone() * direction));
        let y_closed = local[S_Y_CLOSED];
        let x_closed = local[S_X_CLOSED];
        s_bool(builder, y_closed);
        s_bool(builder, x_closed);
        builder.assert_zero(
            y_closed - ((straddle - F::ONE) * (ay_eq - F::ONE) * (by_eq - F::ONE) + F::ONE),
        );
        let x_straddle = ax_gt + bx_gt - ax_gt * bx_gt * f(2);
        builder.assert_zero(
            x_closed - ((x_straddle - F::ONE) * (ax_eq - F::ONE) * (bx_eq - F::ONE) + F::ONE),
        );
        builder.assert_zero(
            (phase - F::ONE) * ozero * y_closed * (dy_zero - (dy_zero - F::ONE) * x_closed),
        );

        let lat_select = s_eq(builder, edge, f(10), local[S_LAT_SELECT], local[S_LAT_INV]);
        let lon_select = s_eq(builder, edge, f(11), local[S_LON_SELECT], local[S_LON_INV]);
        builder.assert_zero(
            (phase - F::ONE) * real * lat_select * (poseidon.inputs[1] - py - f(90_000_000)),
        );
        builder.assert_zero(
            (phase - F::ONE) * real * lon_select * (poseidon.inputs[1] - px - f(180_000_000)),
        );

        {
            let mut first = builder.when_first_row();
            first.assert_zero(phase);
            first.assert_zero(edge);
            first.assert_one(edge_zero);
            first.assert_one(real);
            first.assert_eq(remaining, f((self.events * MAX_POLYGON_VERTICES) as u64));
            first.assert_eq(batch, f(POSEIDON_TAG_EVENT_BATCH));
            first.assert_eq(poly, f(POSEIDON_TAG_POLYGON));
            first.assert_zero(parity);
            first.assert_eq(fx, ax);
            first.assert_eq(fy, ay);
        }
        {
            let mut transition = builder.when_transition();
            transition.assert_zero(next[S_PHASE] + phase - F::ONE);
            transition.assert_eq(next[S_FX], fx);
            transition.assert_eq(next[S_FY], fy);
            transition.assert_zero((phase - F::ONE) * (next[S_EDGE] - edge));
            transition.assert_zero(
                phase * (next[S_EDGE] - edge - F::ONE + last * f(MAX_POLYGON_VERTICES as u64)),
            );
            transition.assert_zero((phase - F::ONE) * (next[S_REAL] - real));
            transition.assert_zero(phase * next[S_REAL] * (real - F::ONE));
            transition.assert_zero((phase - F::ONE) * (next[S_REMAINING] - remaining));
            transition.assert_zero(phase * (next[S_REMAINING] - remaining + real));
            transition.assert_zero((phase - F::ONE) * (next[S_BATCH_STATE] - output));
            transition.assert_zero(phase * (next[S_BATCH_STATE] - batch));
            transition.assert_zero((phase - F::ONE) * (next[S_POLY_STATE] - poly));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_POLY_STATE] - output));
            transition.assert_zero(phase * last * (next[S_POLY_STATE] - f(POSEIDON_TAG_POLYGON)));
            transition.assert_zero((phase - F::ONE) * (next[S_PX] - px));
            transition.assert_zero((phase - F::ONE) * (next[S_PY] - py));
            transition.assert_zero((phase - F::ONE) * (next[S_AX] - ax));
            transition.assert_zero((phase - F::ONE) * (next[S_AY] - ay));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_PX] - px));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_PY] - py));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_AX] - bx));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_AY] - by));
            let updated = parity + cross - parity * cross * f(2);
            transition.assert_zero((phase - F::ONE) * (next[S_PARITY] - updated));
            transition.assert_zero(phase * (last - F::ONE) * (next[S_PARITY] - parity));
            transition.assert_zero(phase * last * next[S_PARITY]);
        }
        builder.assert_zero((phase - F::ONE) * edge_zero * parity);
        builder.assert_zero(phase * last * (bx - fx));
        builder.assert_zero(phase * last * (by - fy));
        builder.assert_zero(phase * last * (output - polygon_root));
        builder.assert_zero(phase * last * parity);
        {
            let mut last_row = builder.when_last_row();
            last_row.assert_one(phase);
            last_row.assert_one(last);
            last_row.assert_eq(remaining, real);
            last_row.assert_eq(batch, event_root);
            last_row.assert_zero(parity);
        }
        let _ = next_poseidon;
    }
}

fn s_bool<AB: AirBuilder<F = F>>(builder: &mut AB, value: AB::Var) {
    builder.assert_zero(value * (value - F::ONE));
}
fn s_range<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    row: &[AB::Var],
    value: AB::Var,
    start: usize,
    bits: usize,
) {
    let mut result = AB::Expr::ZERO;
    for bit in 0..bits {
        s_bool(builder, row[start + bit]);
        result += row[start + bit] * f(1u64 << bit);
    }
    builder.assert_eq(value, result);
}
fn s_eq<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    left: AB::Var,
    right: impl Into<AB::Expr>,
    equal: AB::Var,
    inverse: AB::Var,
) -> AB::Var {
    let difference = left - right.into();
    s_bool(builder, equal);
    builder.assert_zero(equal * difference.clone());
    builder.assert_zero(difference * inverse + equal - F::ONE);
    equal
}
fn s_compare<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    row: &[AB::Var],
    index: usize,
    left: AB::Var,
    right: AB::Var,
) -> AB::Var {
    let start = S_COMPARE_START + index * S_COMPARE_WIDTH;
    let greater = row[start];
    let gd = row[start + 1];
    let od = row[start + 2];
    s_bool(builder, greater);
    s_range(builder, row, gd, start + 3, C1_COMPARE_BITS);
    s_range(
        builder,
        row,
        od,
        start + 3 + C1_COMPARE_BITS,
        C1_COMPARE_BITS,
    );
    builder.assert_zero(greater * (left - right - F::ONE - gd));
    builder.assert_zero((greater - F::ONE) * (right - left - od));
    greater
}

fn prove_c3(lot: &SyntheticLot, events: usize) -> Result<ProofStats> {
    let witness_start = Instant::now();
    validate_c3(lot, events)?;
    let trace = c3_trace(lot, events)?;
    let pis = vec![f(THRESHOLD), f(events as u64)];
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(&C3Air, trace, &pis)?;
    Ok(ProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn validate_c3(lot: &SyntheticLot, events: usize) -> Result<()> {
    if events == 0 {
        bail!("c3 requires at least one event");
    }
    if lot.readings.len() < events || lot.timestamps.len() < events {
        bail!("synthetic lot has fewer rows than requested events");
    }
    for i in 0..events {
        if lot.readings[i] > THRESHOLD {
            bail!("reading overflow at row {i}");
        }
        if lot.readings[i] > C3_MAX_RANGE_VALUE {
            bail!("reading exceeds {RANGE_BITS}-bit range at row {i}");
        }
        let diff = THRESHOLD - lot.readings[i];
        if diff > C3_MAX_RANGE_VALUE {
            bail!("threshold diff exceeds {RANGE_BITS}-bit range at row {i}");
        }
        if i > 0 && lot.timestamps[i] <= lot.timestamps[i - 1] {
            bail!("timestamp is not strictly monotonic at row {i}");
        }
        if i + 1 < events {
            let Some(delta_minus_one) = lot.timestamps[i + 1]
                .checked_sub(lot.timestamps[i])
                .and_then(|delta| delta.checked_sub(1))
            else {
                bail!("timestamp is not strictly monotonic at row {}", i + 1);
            };
            if delta_minus_one > C3_MAX_RANGE_VALUE {
                bail!("timestamp delta exceeds {RANGE_BITS}-bit range at row {i}");
            }
        }
    }
    Ok(())
}

fn c3_trace(lot: &SyntheticLot, events: usize) -> Result<RowMajorMatrix<F>> {
    let rows = events.next_power_of_two().max(2);
    let mut values = vec![F::ZERO; rows * C3_WIDTH];
    for row in 0..rows {
        let src = row.min(events - 1);
        let reading = lot.readings[src];
        let timestamp = lot.timestamps[src];
        let threshold_diff = THRESHOLD - reading;
        let is_real = row < events;
        let transition_enabled = row + 1 < events;
        let time_delta_minus_one = if transition_enabled {
            lot.timestamps[row + 1]
                .checked_sub(timestamp)
                .and_then(|delta| delta.checked_sub(1))
                .ok_or_else(|| anyhow!("timestamp is not strictly monotonic at row {row}"))?
        } else {
            0
        };
        let base = row * C3_WIDTH;
        values[base + C3_READING_COL] = f(reading);
        values[base + C3_TIMESTAMP_COL] = f(timestamp);
        values[base + C3_THRESHOLD_DIFF_COL] = f(threshold_diff);
        values[base + C3_TIME_DELTA_MINUS_ONE_COL] = f(time_delta_minus_one);
        values[base + C3_IS_REAL_COL] = if is_real { F::ONE } else { F::ZERO };
        values[base + C3_TRANSITION_ENABLED_COL] =
            if transition_enabled { F::ONE } else { F::ZERO };
        values[base + C3_REAL_COUNT_COL] = f(row.min(events - 1) as u64 + 1);
        fill_bits(&mut values, base + C3_READING_BITS_START, reading);
        fill_bits(&mut values, base + C3_DIFF_BITS_START, threshold_diff);
        fill_bits(
            &mut values,
            base + C3_DELTA_BITS_START,
            time_delta_minus_one,
        );
    }
    Ok(RowMajorMatrix::new(values, C3_WIDTH))
}

fn fill_bits(values: &mut [F], start: usize, value: u64) {
    for bit in 0..RANGE_BITS {
        values[start + bit] = if ((value >> bit) & 1) == 1 {
            F::ONE
        } else {
            F::ZERO
        };
    }
}

pub struct C3Air;

impl BaseAir<F> for C3Air {
    fn width(&self) -> usize {
        C3_WIDTH
    }

    fn num_public_values(&self) -> usize {
        2
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        Some(2)
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for C3Air {
    fn eval(&self, builder: &mut AB) {
        let main = builder.main();
        let local = main.current_slice();
        let next = main.next_slice();
        let threshold_pub = builder.public_values()[0];
        let events_pub = builder.public_values()[1];
        let threshold_pi: AB::Expr = threshold_pub.into();
        let reading = local[C3_READING_COL];
        let timestamp = local[C3_TIMESTAMP_COL];
        let diff = local[C3_THRESHOLD_DIFF_COL];
        let delta_minus_one = local[C3_TIME_DELTA_MINUS_ONE_COL];
        let is_real = local[C3_IS_REAL_COL];
        let transition_enabled = local[C3_TRANSITION_ENABLED_COL];
        let real_count = local[C3_REAL_COUNT_COL];
        let next_timestamp = next[C3_TIMESTAMP_COL];
        let next_is_real = next[C3_IS_REAL_COL];
        let next_real_count = next[C3_REAL_COUNT_COL];

        builder.assert_zero(is_real * (is_real - F::ONE));
        builder.assert_zero(transition_enabled * (transition_enabled - F::ONE));
        builder.assert_zero(is_real * (reading + diff - threshold_pi));

        assert_range(builder, local, reading, C3_READING_BITS_START);
        assert_range(builder, local, diff, C3_DIFF_BITS_START);
        assert_range(builder, local, delta_minus_one, C3_DELTA_BITS_START);

        {
            let mut first = builder.when_first_row();
            first.assert_one(is_real);
            first.assert_one(real_count);
        }

        {
            let mut transition = builder.when_transition();
            transition.assert_zero(next_is_real * (is_real - F::ONE));
            transition.assert_zero(transition_enabled - is_real * next_is_real);
            transition.assert_eq(next_real_count, real_count + next_is_real);
            transition
                .when(transition_enabled)
                .assert_eq(next_timestamp, timestamp + delta_minus_one + F::ONE);
        }

        {
            let mut last = builder.when_last_row();
            last.assert_zero(transition_enabled);
            last.assert_eq(real_count, events_pub);
        }
    }
}

fn assert_range<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    local: &[AB::Var],
    value: AB::Var,
    bits_start: usize,
) {
    let mut recomposed = AB::Expr::ZERO;
    for bit in 0..RANGE_BITS {
        let limb = local[bits_start + bit];
        let weight = f(1u64 << bit);
        builder.assert_zero(limb * (limb - F::ONE));
        recomposed += limb * weight;
    }
    builder.assert_zero(value - recomposed);
}

pub struct C2MerkleAir;

impl BaseAir<F> for C2MerkleAir {
    fn width(&self) -> usize {
        C2_WIDTH
    }

    fn main_next_row_columns(&self) -> Vec<usize> {
        vec![]
    }

    fn num_public_values(&self) -> usize {
        1
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        None
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for C2MerkleAir {
    fn eval(&self, builder: &mut AB) {
        let root_pub = builder.public_values()[0];
        let main = builder.main();
        let local = main.current_slice();

        for lane in 0..C2_VECTOR_LANES {
            let cols = poseidon_lane::<AB>(local, lane);
            eval_poseidon2_cols(builder, cols);
        }

        let leaf = poseidon_lane::<AB>(local, 0);
        leaf_assert_event_tuple(builder, leaf);
        let mut previous_output = leaf.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];

        for level in 0..STRICT_MERKLE_DEPTH {
            let bit = local[C2_INDEX_BITS_START + level];
            let path = poseidon_lane::<AB>(local, level + 1);
            builder.assert_zero(bit * (bit - F::ONE));
            builder.assert_eq(path.inputs[2], f(POSEIDON_TAG_CERT));
            builder.assert_zero(path.inputs[3]);
            builder.assert_zero(path.inputs[4]);
            builder.assert_zero(path.inputs[5]);
            builder.assert_zero(path.inputs[6]);
            builder.assert_eq(path.inputs[7], f(3));
            builder.assert_zero((bit - F::ONE) * (path.inputs[0] - previous_output));
            builder.assert_zero(bit * (path.inputs[1] - previous_output));
            previous_output = path.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
        }

        builder.assert_eq(previous_output, root_pub);
    }
}

pub struct C5NullifierUpdateAir;

impl BaseAir<F> for C5NullifierUpdateAir {
    fn width(&self) -> usize {
        C5_WIDTH
    }

    fn main_next_row_columns(&self) -> Vec<usize> {
        vec![]
    }

    fn num_public_values(&self) -> usize {
        4
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        None
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for C5NullifierUpdateAir {
    fn eval(&self, builder: &mut AB) {
        let old_root: AB::Expr = builder.public_values()[0].into();
        let new_root: AB::Expr = builder.public_values()[1].into();
        let nullifier_pub: AB::Expr = builder.public_values()[2].into();
        let state: AB::Expr = builder.public_values()[3].into();
        let main = builder.main();
        let local = main.current_slice();

        for lane in 0..C5_VECTOR_LANES {
            eval_poseidon2_cols(builder, poseidon_lane::<AB>(local, lane));
        }

        builder.assert_eq(state.clone(), F::ONE);

        let nullifier = poseidon_lane::<AB>(local, 0);
        builder.assert_eq(nullifier.inputs[2], f(POSEIDON_TAG_NULLIFIER));
        for input in &nullifier.inputs[3..7] {
            builder.assert_zero(*input);
        }
        builder.assert_eq(nullifier.inputs[7], f(3));
        let nullifier_output: AB::Expr =
            nullifier.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();
        builder.assert_eq(nullifier_output, nullifier_pub.clone());

        let index_hash = poseidon_lane::<AB>(local, 1);
        builder.assert_eq(index_hash.inputs[0], nullifier_pub);
        builder.assert_eq(index_hash.inputs[1], f(POSEIDON_TAG_NULLIFIER_INDEX));
        for input in &index_hash.inputs[2..7] {
            builder.assert_zero(*input);
        }
        builder.assert_eq(index_hash.inputs[7], f(2));
        let index_hash_output: AB::Expr =
            index_hash.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();

        let mut index = AB::Expr::ZERO;
        let mut quotient = AB::Expr::ZERO;
        for bit in 0..C5_SPARSE_DEPTH {
            let index_bit = local[C5_INDEX_BITS_START + bit];
            let quotient_bit = local[C5_QUOTIENT_BITS_START + bit];
            builder.assert_zero(index_bit * (index_bit - F::ONE));
            builder.assert_zero(quotient_bit * (quotient_bit - F::ONE));
            index += index_bit * f(1u64 << bit);
            quotient += quotient_bit * f(1u64 << bit);
        }
        builder.assert_eq(
            index_hash_output,
            index + quotient * f(1u64 << C5_SPARSE_DEPTH),
        );

        let old_leaf = poseidon_lane::<AB>(local, 2);
        builder.assert_zero(old_leaf.inputs[0]);
        builder.assert_eq(old_leaf.inputs[1], f(POSEIDON_TAG_EMPTY));
        for input in &old_leaf.inputs[2..7] {
            builder.assert_zero(*input);
        }
        builder.assert_eq(old_leaf.inputs[7], f(2));

        let new_leaf = poseidon_lane::<AB>(local, 3);
        builder.assert_eq(new_leaf.inputs[0], state);
        builder.assert_eq(new_leaf.inputs[1], f(POSEIDON_TAG_EMPTY));
        for input in &new_leaf.inputs[2..7] {
            builder.assert_zero(*input);
        }
        builder.assert_eq(new_leaf.inputs[7], f(2));

        let mut previous_old: AB::Expr =
            old_leaf.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();
        let mut previous_new: AB::Expr =
            new_leaf.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();
        for level in 0..C5_SPARSE_DEPTH {
            let bit = local[C5_INDEX_BITS_START + level];
            let sibling = local[C5_SIBLINGS_START + level];
            let old_path = poseidon_lane::<AB>(local, 4 + level * 2);
            let new_path = poseidon_lane::<AB>(local, 5 + level * 2);
            for path in [old_path, new_path] {
                builder.assert_eq(path.inputs[2], f(POSEIDON_TAG_EMPTY));
                for input in &path.inputs[3..7] {
                    builder.assert_zero(*input);
                }
                builder.assert_eq(path.inputs[7], f(3));
            }
            let old_left = previous_old.clone() + bit * (sibling - previous_old.clone());
            let old_right = sibling + bit * (previous_old.clone() - sibling);
            let new_left = previous_new.clone() + bit * (sibling - previous_new.clone());
            let new_right = sibling + bit * (previous_new.clone() - sibling);
            builder.assert_eq(old_path.inputs[0], old_left);
            builder.assert_eq(old_path.inputs[1], old_right);
            builder.assert_eq(new_path.inputs[0], new_left);
            builder.assert_eq(new_path.inputs[1], new_right);
            previous_old = old_path.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();
            previous_new = new_path.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0].into();
        }
        builder.assert_eq(previous_old, old_root);
        builder.assert_eq(previous_new, new_root);
    }
}

fn poseidon_lane<'a, AB: AirBuilder<F = F>>(
    local: &'a [AB::Var],
    lane: usize,
) -> &'a Poseidon2Cols<
    AB::Var,
    POSEIDON_WIDTH,
    GOLDILOCKS_SBOX_DEGREE,
    SBOX_REGISTERS,
    HALF_FULL_ROUNDS,
    PARTIAL_ROUNDS,
> {
    let start = lane * POSEIDON_COLS;
    (&local[start..start + POSEIDON_COLS]).borrow()
}

fn leaf_assert_event_tuple<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    leaf: &Poseidon2Cols<
        AB::Var,
        POSEIDON_WIDTH,
        GOLDILOCKS_SBOX_DEGREE,
        SBOX_REGISTERS,
        HALF_FULL_ROUNDS,
        PARTIAL_ROUNDS,
    >,
) {
    builder.assert_eq(leaf.inputs[5], f(POSEIDON_TAG_CERT));
    builder.assert_zero(leaf.inputs[6]);
    builder.assert_eq(leaf.inputs[7], f(6));
}

fn eval_poseidon2_cols<AB: AirBuilder<F = F>>(
    builder: &mut AB,
    local: &Poseidon2Cols<
        AB::Var,
        POSEIDON_WIDTH,
        GOLDILOCKS_SBOX_DEGREE,
        SBOX_REGISTERS,
        HALF_FULL_ROUNDS,
        PARTIAL_ROUNDS,
    >,
) {
    let mut state: [_; POSEIDON_WIDTH] = local.inputs.map(|x| x.into());
    GenericPoseidon2LinearLayersGoldilocks::external_linear_layer(&mut state);

    for (round, constants) in GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_INITIAL
        .iter()
        .enumerate()
    {
        eval_poseidon2_full_round::<AB>(
            &mut state,
            &local.beginning_full_rounds[round],
            constants,
            builder,
        );
    }

    for (partial_round, constant) in local
        .partial_rounds
        .iter()
        .zip(GOLDILOCKS_POSEIDON2_RC_8_INTERNAL.iter())
    {
        eval_poseidon2_partial_round::<AB>(&mut state, partial_round, constant, builder);
    }

    for (round, constants) in GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_FINAL.iter().enumerate() {
        eval_poseidon2_full_round::<AB>(
            &mut state,
            &local.ending_full_rounds[round],
            constants,
            builder,
        );
    }
}

fn eval_poseidon2_full_round<AB: AirBuilder<F = F>>(
    state: &mut [AB::Expr; POSEIDON_WIDTH],
    full_round: &FullRound<AB::Var, POSEIDON_WIDTH, GOLDILOCKS_SBOX_DEGREE, SBOX_REGISTERS>,
    round_constants: &[F; POSEIDON_WIDTH],
    builder: &mut AB,
) {
    for (i, (s, r)) in state.iter_mut().zip(round_constants.iter()).enumerate() {
        *s += r.dup();
        eval_poseidon2_sbox::<AB>(&full_round.sbox[i], s, builder);
    }
    GenericPoseidon2LinearLayersGoldilocks::external_linear_layer(state);
    for (state_i, post_i) in state.iter_mut().zip(full_round.post) {
        builder.assert_eq(state_i.dup(), post_i);
        *state_i = post_i.into();
    }
}

fn eval_poseidon2_partial_round<AB: AirBuilder<F = F>>(
    state: &mut [AB::Expr; POSEIDON_WIDTH],
    partial_round: &PartialRound<AB::Var, POSEIDON_WIDTH, GOLDILOCKS_SBOX_DEGREE, SBOX_REGISTERS>,
    round_constant: &F,
    builder: &mut AB,
) {
    state[0] += round_constant.dup();
    eval_poseidon2_sbox::<AB>(&partial_round.sbox, &mut state[0], builder);
    builder.assert_eq(state[0].dup(), partial_round.post_sbox);
    state[0] = partial_round.post_sbox.into();
    GenericPoseidon2LinearLayersGoldilocks::internal_linear_layer(state);
}

fn eval_poseidon2_sbox<AB: AirBuilder<F = F>>(
    sbox: &SBox<AB::Var, GOLDILOCKS_SBOX_DEGREE, SBOX_REGISTERS>,
    x: &mut AB::Expr,
    builder: &mut AB,
) {
    match (GOLDILOCKS_SBOX_DEGREE, SBOX_REGISTERS) {
        (7, 1) => {
            let committed_x3 = sbox.0[0].into();
            builder.assert_eq(committed_x3.dup(), x.cube());
            *x = committed_x3.square() * x.dup();
        }
        (7, 0) => *x = x.exp_const_u64::<7>(),
        _ => unreachable!("unsupported Poseidon2 S-box configuration"),
    }
}

fn prove_c4(lot: &SyntheticLot) -> Result<ProofStats> {
    let witness_start = Instant::now();
    let (trace, pis) = c4_trace_and_pis(lot, None)?;
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;
    let air = PoseidonStatementAir {
        public_input_indices: [Some(0), None, Some(2), None],
        num_pis: 3,
    };
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(&air, trace, &pis)?;
    Ok(ProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn c4_trace_and_pis(
    lot: &SyntheticLot,
    tamper: Option<C4Tamper>,
) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    let actor_id = f(lot.actor_id);
    let actor_secret = f(lot.actor_secret);
    let role_tag = f(lot.role_tag);
    let input = c4_poseidon2_input(actor_id, actor_secret, role_tag);
    let commitment = poseidon2_permute(input)[0];
    let trace = poseidon_trace(vec![input]);
    let mut pis = vec![actor_id, role_tag, commitment];
    match tamper {
        Some(C4Tamper::WrongSecret) => {
            let bad = c4_poseidon2_input(actor_id, actor_secret + F::ONE, role_tag);
            return Ok((poseidon_trace(vec![bad]), pis));
        }
        Some(C4Tamper::WrongRole) => pis[1] += F::ONE,
        Some(C4Tamper::WrongCommitment) => pis[2] += F::ONE,
        None => {}
    }
    Ok((trace, pis))
}

fn c4_poseidon2_input(actor_id: F, actor_secret: F, role_tag: F) -> [F; POSEIDON_WIDTH] {
    [
        actor_id,
        actor_secret,
        role_tag,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(3),
    ]
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum C4Tamper {
    WrongSecret,
    WrongRole,
    WrongCommitment,
}

fn prove_c2(lot: &SyntheticLot, seed: usize, events: usize) -> Result<ProofStats> {
    let witness_start = Instant::now();
    let (trace, pis) = c2_trace_and_pis(lot, seed, events, None)?;
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(&C2MerkleAir, trace, &pis)?;
    Ok(ProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn c2_trace_and_pis(
    lot: &SyntheticLot,
    seed: usize,
    events: usize,
    tamper: Option<PathTamper>,
) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    let witness = c2_path_witness(lot, seed, events, tamper)?;
    Ok((
        c2_trace(witness.inputs, &witness.index_bits),
        vec![witness.root],
    ))
}

struct C2PathWitness {
    inputs: Vec<[F; POSEIDON_WIDTH]>,
    index_bits: Vec<F>,
    root: F,
}

fn c2_path_witness(
    lot: &SyntheticLot,
    seed: usize,
    events: usize,
    tamper: Option<PathTamper>,
) -> Result<C2PathWitness> {
    if events == 0 || events > STRICT_MERKLE_LEAVES {
        bail!("c2 requires 1..={STRICT_MERKLE_LEAVES} events");
    }
    if lot.readings.len() < events || lot.timestamps.len() < events || lot.cert_ids.len() < events {
        bail!("synthetic lot has fewer C2 rows than requested events");
    }

    let event_index = seed % events;
    let mut leaves = (0..STRICT_MERKLE_LEAVES)
        .map(|i| c2_leaf_hash(lot, i, events))
        .collect::<Vec<_>>();
    let leaf_input = c2_event_leaf_input(lot, event_index);
    let mut current = poseidon2_permute(leaf_input)[0];
    let mut inputs = Vec::with_capacity(STRICT_MERKLE_DEPTH + 1);
    let mut index_bits = Vec::with_capacity(STRICT_MERKLE_DEPTH);
    inputs.push(leaf_input);

    let mut idx = event_index;
    while leaves.len() > 1 {
        let bit = idx & 1;
        let sibling_idx = if bit == 0 { idx + 1 } else { idx - 1 };
        let sibling = leaves[sibling_idx];
        let parent_input = if bit == 0 {
            merkle_parent_input(current, sibling, f(POSEIDON_TAG_CERT))
        } else {
            merkle_parent_input(sibling, current, f(POSEIDON_TAG_CERT))
        };
        current = poseidon2_permute(parent_input)[0];
        inputs.push(parent_input);
        index_bits.push(f(bit as u64));
        leaves = leaves
            .chunks(2)
            .map(|pair| {
                poseidon2_permute(merkle_parent_input(pair[0], pair[1], f(POSEIDON_TAG_CERT)))[0]
            })
            .collect();
        idx /= 2;
    }

    let mut root = current;
    match tamper {
        Some(PathTamper::WrongRoot) => root += F::ONE,
        Some(PathTamper::WrongPath) => inputs[1][1] += F::ONE,
        Some(PathTamper::WrongIndex) => {
            if let Some(first) = index_bits.first_mut() {
                *first = F::ONE - *first;
            }
        }
        None => {}
    }
    Ok(C2PathWitness {
        inputs,
        index_bits,
        root,
    })
}

fn c2_trace(inputs: Vec<[F; POSEIDON_WIDTH]>, index_bits: &[F]) -> RowMajorMatrix<F> {
    debug_assert_eq!(inputs.len(), STRICT_MERKLE_DEPTH + 1);
    debug_assert_eq!(index_bits.len(), STRICT_MERKLE_DEPTH);
    let poseidon = poseidon_trace(inputs);
    debug_assert_eq!(poseidon.values.len(), POSEIDON_COLS * C2_VECTOR_LANES);
    let mut values = Vec::with_capacity(C2_WIDTH);
    values.extend_from_slice(&poseidon.values);
    values.extend_from_slice(index_bits);
    RowMajorMatrix::new(values, C2_WIDTH)
}

fn c2_event_leaf_input(lot: &SyntheticLot, event_index: usize) -> [F; POSEIDON_WIDTH] {
    [
        f(lot.lot_id),
        f(event_index as u64),
        f(lot.cert_ids[event_index]),
        f(lot.readings[event_index]),
        f(lot.timestamps[event_index]),
        f(POSEIDON_TAG_CERT),
        F::ZERO,
        f(6),
    ]
}

fn c2_filler_leaf_input(lot: &SyntheticLot, index: usize) -> [F; POSEIDON_WIDTH] {
    [
        f(lot.lot_id),
        f(index as u64),
        f(50_000_000 + lot.lot_id + index as u64),
        F::ZERO,
        F::ZERO,
        f(POSEIDON_TAG_CERT),
        F::ZERO,
        f(6),
    ]
}

fn c2_leaf_hash(lot: &SyntheticLot, index: usize, events: usize) -> F {
    let input = if index < events {
        c2_event_leaf_input(lot, index)
    } else {
        c2_filler_leaf_input(lot, index)
    };
    poseidon2_permute(input)[0]
}

fn merkle_parent_input(left: F, right: F, tag: F) -> [F; POSEIDON_WIDTH] {
    [left, right, tag, F::ZERO, F::ZERO, F::ZERO, F::ZERO, f(3)]
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum PathTamper {
    WrongRoot,
    WrongPath,
    WrongIndex,
}

fn prove_c5(lot: &SyntheticLot) -> Result<ProofStats> {
    let witness_start = Instant::now();
    let (trace, pis) = c5_trace_and_pis(lot, None)?;
    let witness_ms = witness_start.elapsed().as_secs_f64() * 1000.0;
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(&C5NullifierUpdateAir, trace, &pis)?;
    Ok(ProofStats {
        witness_ms,
        prove_ms,
        verify_ms,
        proof_bytes,
    })
}

fn c5_trace_and_pis(
    lot: &SyntheticLot,
    tamper: Option<C5Tamper>,
) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    c5_trace_and_pis_from_witness(c5_witness(lot, tamper)?)
}

#[cfg(test)]
fn c5_trace_and_pis_after_prior_nullifier(
    lot: &SyntheticLot,
    prior_lot: &SyntheticLot,
) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    c5_trace_and_pis_from_witness(c5_witness_after_prior_nullifier(lot, prior_lot)?)
}

#[cfg(test)]
fn c5_trace_and_pis_replay(lot: &SyntheticLot) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    let nullifier = poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0];
    let index_hash_u64 = poseidon2_permute(c5_index_input(nullifier))[0].as_canonical_u64();
    let index = index_hash_u64 as u32;
    let defaults = c5_default_nodes();
    let mut old_root = poseidon2_permute(c5_leaf_input(F::ONE))[0];
    for level in 0..C5_SPARSE_DEPTH {
        old_root = poseidon2_permute(c5_parent_input(
            old_root,
            defaults[level],
            (index >> level) & 1,
        ))[0];
    }
    c5_trace_and_pis_from_witness(c5_witness_from_path(
        lot,
        index_hash_u64,
        old_root,
        defaults[..C5_SPARSE_DEPTH].to_vec(),
        F::ONE,
    )?)
}

fn c5_trace_and_pis_from_witness(witness: C5Witness) -> Result<(RowMajorMatrix<F>, Vec<F>)> {
    Ok((
        c5_trace(
            witness.inputs,
            &witness.index_bits,
            &witness.quotient_bits,
            &witness.siblings,
        ),
        vec![
            witness.old_root,
            witness.new_root,
            witness.nullifier,
            witness.state,
        ],
    ))
}

struct C5Witness {
    inputs: Vec<[F; POSEIDON_WIDTH]>,
    index_bits: Vec<F>,
    quotient_bits: Vec<F>,
    siblings: Vec<F>,
    old_root: F,
    new_root: F,
    nullifier: F,
    state: F,
}

fn c5_witness(lot: &SyntheticLot, tamper: Option<C5Tamper>) -> Result<C5Witness> {
    let nullifier_input = match tamper {
        Some(C5Tamper::WrongSecret) => c5_nullifier_input(lot.lot_id, lot.secret + 1),
        _ => c5_nullifier_input(lot.lot_id, lot.secret),
    };
    let nullifier = poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0];
    let index_hash = poseidon2_permute(c5_index_input(nullifier))[0];
    let index_hash_u64 = index_hash.as_canonical_u64();
    let index = index_hash_u64 as u32;
    let quotient = (index_hash_u64 >> C5_SPARSE_DEPTH) as u32;
    let mut index_bits = c5_bits(index as u64);
    let quotient_bits = c5_bits(quotient as u64);
    let defaults = c5_default_nodes();
    let old_root = defaults[C5_SPARSE_DEPTH];
    let mut state = F::ONE;
    let old_leaf_input = match tamper {
        Some(C5Tamper::Duplicate) => c5_leaf_input(F::ONE),
        _ => c5_leaf_input(F::ZERO),
    };
    let new_leaf_input = c5_leaf_input(F::ONE);
    let mut old_current = poseidon2_permute(old_leaf_input)[0];
    let mut new_current = poseidon2_permute(new_leaf_input)[0];
    let mut inputs = Vec::with_capacity(4 + C5_SPARSE_DEPTH * 2);
    let mut siblings = Vec::with_capacity(C5_SPARSE_DEPTH);
    inputs.extend([
        nullifier_input,
        c5_index_input(nullifier),
        old_leaf_input,
        new_leaf_input,
    ]);

    for level in 0..C5_SPARSE_DEPTH {
        let bit = (index >> level) & 1;
        let sibling = defaults[level];
        let old_parent = c5_parent_input(old_current, sibling, bit);
        let new_parent = c5_parent_input(new_current, sibling, bit);
        old_current = poseidon2_permute(old_parent)[0];
        new_current = poseidon2_permute(new_parent)[0];
        inputs.extend([old_parent, new_parent]);
        siblings.push(sibling);
    }

    let mut new_root = new_current;
    match tamper {
        Some(C5Tamper::WrongOldRoot) => {
            return Ok(C5Witness {
                inputs,
                index_bits,
                quotient_bits,
                siblings,
                old_root: old_root + F::ONE,
                new_root,
                nullifier,
                state,
            });
        }
        Some(C5Tamper::WrongNewRoot) => new_root += F::ONE,
        Some(C5Tamper::WrongPath) => inputs[4][1] += F::ONE,
        Some(C5Tamper::WrongIndex) => index_bits[0] = F::ONE - index_bits[0],
        Some(C5Tamper::WrongState) => state = F::ZERO,
        Some(C5Tamper::Duplicate) | Some(C5Tamper::WrongSecret) | None => {}
    }
    Ok(C5Witness {
        inputs,
        index_bits,
        quotient_bits,
        siblings,
        old_root,
        new_root,
        nullifier,
        state,
    })
}

#[cfg(test)]
fn c5_witness_after_prior_nullifier(
    lot: &SyntheticLot,
    prior_lot: &SyntheticLot,
) -> Result<C5Witness> {
    let nullifier = poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0];
    let index_hash = poseidon2_permute(c5_index_input(nullifier))[0];
    let index_hash_u64 = index_hash.as_canonical_u64();
    let index = index_hash_u64 as u32;
    let prior_nullifier =
        poseidon2_permute(c5_nullifier_input(prior_lot.lot_id, prior_lot.secret))[0];
    let prior_index =
        poseidon2_permute(c5_index_input(prior_nullifier))[0].as_canonical_u64() as u32;
    if index == prior_index {
        bail!("prior nullifier collides with the fresh nullifier's 32-bit index");
    }

    let defaults = c5_default_nodes();
    let mut prior_nodes = Vec::with_capacity(C5_SPARSE_DEPTH + 1);
    let mut prior_current = poseidon2_permute(c5_leaf_input(F::ONE))[0];
    prior_nodes.push(prior_current);
    for level in 0..C5_SPARSE_DEPTH {
        let bit = (prior_index >> level) & 1;
        prior_current = poseidon2_permute(c5_parent_input(prior_current, defaults[level], bit))[0];
        prior_nodes.push(prior_current);
    }

    let mut siblings = Vec::with_capacity(C5_SPARSE_DEPTH);
    for level in 0..C5_SPARSE_DEPTH {
        let target_sibling = (index >> level) ^ 1;
        let prior_node = prior_index >> level;
        siblings.push(if target_sibling == prior_node {
            prior_nodes[level]
        } else {
            defaults[level]
        });
    }

    c5_witness_from_path(
        lot,
        index_hash_u64,
        prior_nodes[C5_SPARSE_DEPTH],
        siblings,
        F::ZERO,
    )
}

#[cfg(test)]
fn c5_witness_from_path(
    lot: &SyntheticLot,
    index_hash_u64: u64,
    old_root: F,
    siblings: Vec<F>,
    old_leaf_state: F,
) -> Result<C5Witness> {
    let index = index_hash_u64 as u32;
    let quotient = (index_hash_u64 >> C5_SPARSE_DEPTH) as u32;
    let index_bits = c5_bits(index as u64);
    let quotient_bits = c5_bits(quotient as u64);
    let old_leaf_input = c5_leaf_input(old_leaf_state);
    let new_leaf_input = c5_leaf_input(F::ONE);
    let mut old_current = poseidon2_permute(old_leaf_input)[0];
    let mut new_current = poseidon2_permute(new_leaf_input)[0];
    let mut inputs = Vec::with_capacity(4 + C5_SPARSE_DEPTH * 2);
    inputs.extend([
        c5_nullifier_input(lot.lot_id, lot.secret),
        c5_index_input(poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0]),
        old_leaf_input,
        new_leaf_input,
    ]);

    for (level, sibling) in siblings.iter().copied().enumerate() {
        let bit = (index >> level) & 1;
        let old_parent = c5_parent_input(old_current, sibling, bit);
        let new_parent = c5_parent_input(new_current, sibling, bit);
        old_current = poseidon2_permute(old_parent)[0];
        new_current = poseidon2_permute(new_parent)[0];
        inputs.extend([old_parent, new_parent]);
    }

    if old_current != old_root {
        bail!("sparse-nullifier path does not bind to the supplied old root");
    }

    Ok(C5Witness {
        inputs,
        index_bits,
        quotient_bits,
        siblings,
        old_root,
        new_root: new_current,
        nullifier: poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0],
        state: F::ONE,
    })
}

fn c5_trace(
    inputs: Vec<[F; POSEIDON_WIDTH]>,
    index_bits: &[F],
    quotient_bits: &[F],
    siblings: &[F],
) -> RowMajorMatrix<F> {
    debug_assert_eq!(inputs.len(), 4 + C5_SPARSE_DEPTH * 2);
    debug_assert_eq!(index_bits.len(), C5_SPARSE_DEPTH);
    debug_assert_eq!(quotient_bits.len(), C5_SPARSE_DEPTH);
    debug_assert_eq!(siblings.len(), C5_SPARSE_DEPTH);
    let poseidon = poseidon_trace(inputs);
    debug_assert_eq!(poseidon.values.len(), POSEIDON_COLS * C5_VECTOR_LANES);
    let mut values = Vec::with_capacity(C5_WIDTH);
    values.extend_from_slice(&poseidon.values);
    values.extend_from_slice(index_bits);
    values.extend_from_slice(quotient_bits);
    values.extend_from_slice(siblings);
    RowMajorMatrix::new(values, C5_WIDTH)
}

fn c5_nullifier_input(lot_id: u64, secret: u64) -> [F; POSEIDON_WIDTH] {
    [
        f(lot_id),
        f(secret),
        f(POSEIDON_TAG_NULLIFIER),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(3),
    ]
}

fn c5_index_input(nullifier: F) -> [F; POSEIDON_WIDTH] {
    [
        nullifier,
        f(POSEIDON_TAG_NULLIFIER_INDEX),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(2),
    ]
}

fn c5_leaf_input(state: F) -> [F; POSEIDON_WIDTH] {
    [
        state,
        f(POSEIDON_TAG_EMPTY),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(2),
    ]
}

fn c5_parent_input(current: F, sibling: F, bit: u32) -> [F; POSEIDON_WIDTH] {
    if bit == 0 {
        merkle_parent_input(current, sibling, f(POSEIDON_TAG_EMPTY))
    } else {
        merkle_parent_input(sibling, current, f(POSEIDON_TAG_EMPTY))
    }
}

fn c5_default_nodes() -> Vec<F> {
    let mut current = poseidon2_permute(c5_leaf_input(F::ZERO))[0];
    let mut nodes = Vec::with_capacity(C5_SPARSE_DEPTH + 1);
    nodes.push(current);
    for _ in 0..C5_SPARSE_DEPTH {
        current =
            poseidon2_permute(merkle_parent_input(current, current, f(POSEIDON_TAG_EMPTY)))[0];
        nodes.push(current);
    }
    nodes
}

fn c5_bits(value: u64) -> Vec<F> {
    (0..C5_SPARSE_DEPTH)
        .map(|bit| {
            if value >> bit & 1 == 1 {
                F::ONE
            } else {
                F::ZERO
            }
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum C5Tamper {
    Duplicate,
    WrongOldRoot,
    WrongNewRoot,
    WrongPath,
    WrongIndex,
    WrongSecret,
    WrongState,
}

fn poseidon_trace(inputs: Vec<[F; POSEIDON_WIDTH]>) -> RowMajorMatrix<F> {
    let rows = inputs.len().next_power_of_two().max(1);
    let mut padded = inputs;
    while padded.len() < rows {
        padded.push([F::ZERO; POSEIDON_WIDTH]);
    }
    generate_trace_rows::<
        F,
        GenericPoseidon2LinearLayersGoldilocks,
        POSEIDON_WIDTH,
        GOLDILOCKS_SBOX_DEGREE,
        SBOX_REGISTERS,
        HALF_FULL_ROUNDS,
        PARTIAL_ROUNDS,
    >(padded, &poseidon_constants(), 0)
}

pub struct PoseidonStatementAir {
    public_input_indices: [Option<usize>; 4],
    num_pis: usize,
}

impl BaseAir<F> for PoseidonStatementAir {
    fn width(&self) -> usize {
        p3_poseidon2_air::num_cols::<
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        >()
    }

    fn main_next_row_columns(&self) -> Vec<usize> {
        vec![]
    }

    fn num_public_values(&self) -> usize {
        self.num_pis
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        None
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for PoseidonStatementAir {
    fn eval(&self, builder: &mut AB) {
        let air = Poseidon2Air::<
            F,
            GenericPoseidon2LinearLayersGoldilocks,
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        >::new(poseidon_constants());
        Air::eval(&air, builder);
        let main = builder.main();
        let local: &Poseidon2Cols<
            AB::Var,
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        > = main.current_slice().borrow();
        let pis = builder.public_values();
        if self.num_pis == 0 {
            return;
        }
        let pi0 = pis[0];
        let pi1 = if self.num_pis > 1 { pis[1] } else { pis[0] };
        let pi2 = if self.num_pis > 2 { pis[2] } else { pis[0] };
        if self.num_pis == 1 {
            let out = local.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
            builder.when_first_row().assert_eq(out, pi0);
            return;
        }
        if let Some(input_idx) = self.public_input_indices[0] {
            builder
                .when_first_row()
                .assert_eq(local.inputs[input_idx], pi0);
        }
        if let Some(input_idx) = self.public_input_indices[2] {
            builder
                .when_first_row()
                .assert_eq(local.inputs[input_idx], pi1);
        }
        let out = local.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
        builder.when_first_row().assert_eq(out, pi2);
    }
}

pub struct PoseidonPathAir {
    expected_rows: usize,
}

impl BaseAir<F> for PoseidonPathAir {
    fn width(&self) -> usize {
        p3_poseidon2_air::num_cols::<
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        >()
    }

    fn num_public_values(&self) -> usize {
        0
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        Some(GOLDILOCKS_SBOX_DEGREE as usize)
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for PoseidonPathAir {
    fn eval(&self, builder: &mut AB) {
        let air = Poseidon2Air::<
            F,
            GenericPoseidon2LinearLayersGoldilocks,
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        >::new(poseidon_constants());
        Air::eval(&air, builder);
        let main = builder.main();
        let local: &Poseidon2Cols<
            AB::Var,
            POSEIDON_WIDTH,
            GOLDILOCKS_SBOX_DEGREE,
            SBOX_REGISTERS,
            HALF_FULL_ROUNDS,
            PARTIAL_ROUNDS,
        > = main.current_slice().borrow();
        let _out = local.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
        let _ = self.expected_rows;
    }
}

pub fn invalid_c1_outside(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, WitnessBundle)> {
    let template = build_template(CircuitKind::C1, events, profile)?;
    let lot = crate::synthetic::inside_forbidden_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?;
    Ok((template, witness))
}

pub fn invalid_c3_overflow(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, WitnessBundle)> {
    let template = build_template(CircuitKind::C3, events, profile)?;
    let lot = crate::synthetic::threshold_overflow_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?;
    Ok((template, witness))
}

pub fn invalid_c4_tampered(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, WitnessBundle)> {
    let template = build_template(CircuitKind::C4, events, profile)?;
    let lot = tampered_actor_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?;
    Ok((template, witness))
}

pub fn invalid_c5_duplicate(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, WitnessBundle)> {
    let template = build_template(CircuitKind::C5, events, profile)?;
    let lot = crate::synthetic::synthetic_lot(3, events, profile);
    let start = Instant::now();
    let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::Duplicate))?;
    assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
    Ok((
        template,
        WitnessBundle {
            witness_ms: start.elapsed().as_secs_f64() * 1000.0,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{
        equal_timestamp_lot, non_monotonic_timestamp_lot, synthetic_lot, threshold_overflow_lot,
    };

    #[test]
    fn c3_valid_threshold_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(3, 8, profile);
        let template = build_template(CircuitKind::C3, 8, profile)?;
        prove_and_verify(&template, &lot, 3, profile)?;
        Ok(())
    }

    #[test]
    fn c3_threshold_overflow_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = threshold_overflow_lot(3, 8, profile);
        let template = build_template(CircuitKind::C3, 8, profile)?;
        assert!(prove_and_verify(&template, &lot, 3, profile).is_err());
        Ok(())
    }

    #[test]
    fn c3_non_monotonic_timestamp_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = non_monotonic_timestamp_lot(3, 8, profile);
        let template = build_template(CircuitKind::C3, 8, profile)?;
        assert!(prove_and_verify(&template, &lot, 3, profile).is_err());
        Ok(())
    }

    #[test]
    fn c3_timestamp_equal_previous_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = equal_timestamp_lot(3, 8, profile);
        let template = build_template(CircuitKind::C3, 8, profile)?;
        assert!(prove_and_verify(&template, &lot, 3, profile).is_err());
        Ok(())
    }

    #[test]
    fn c3_padding_does_not_break_valid_trace() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(3, 9, profile);
        let template = build_template(CircuitKind::C3, 9, profile)?;
        prove_and_verify(&template, &lot, 3, profile)?;
        Ok(())
    }

    #[test]
    fn c4_valid_actor_authorization_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(4, 8, profile);
        let template = build_template(CircuitKind::C4, 8, profile)?;
        prove_and_verify(&template, &lot, 4, profile)?;
        Ok(())
    }

    #[test]
    fn c4_public_inputs_are_actor_role_and_commitment_only() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(4, 8, profile);
        let (_, pis) = c4_trace_and_pis(&lot, None)?;
        let actor_id = f(lot.actor_id);
        let actor_secret = f(lot.actor_secret);
        let role_tag = f(lot.role_tag);
        let commitment = poseidon2_permute(c4_poseidon2_input(actor_id, actor_secret, role_tag))[0];

        assert_eq!(pis, vec![actor_id, role_tag, commitment]);
        assert_ne!(pis[0], actor_secret);
        assert_ne!(pis[1], actor_secret);
        Ok(())
    }

    #[test]
    fn c4_wrong_secret_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(4, 8, profile);
        let (trace, pis) = c4_trace_and_pis(&lot, Some(C4Tamper::WrongSecret))?;
        let air = PoseidonStatementAir {
            public_input_indices: [Some(0), None, Some(2), None],
            num_pis: 3,
        };
        assert!(prove_stark(&air, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c4_wrong_role_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(4, 8, profile);
        let (trace, pis) = c4_trace_and_pis(&lot, Some(C4Tamper::WrongRole))?;
        let air = PoseidonStatementAir {
            public_input_indices: [Some(0), None, Some(2), None],
            num_pis: 3,
        };
        assert!(prove_stark(&air, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c4_wrong_commitment_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(4, 8, profile);
        let (trace, pis) = c4_trace_and_pis(&lot, Some(C4Tamper::WrongCommitment))?;
        let air = PoseidonStatementAir {
            public_input_indices: [Some(0), None, Some(2), None],
            num_pis: 3,
        };
        assert!(prove_stark(&air, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c2_valid_merkle_path_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(2, 8, profile);
        let template = build_template(CircuitKind::C2, 8, profile)?;
        prove_and_verify(&template, &lot, 2, profile)?;
        Ok(())
    }

    #[test]
    fn c2_odd_index_merkle_path_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(1, 8, profile);
        let template = build_template(CircuitKind::C2, 8, profile)?;
        prove_and_verify(&template, &lot, 1, profile)?;
        Ok(())
    }

    #[test]
    fn c2_wrong_root_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(2, 8, profile);
        let (trace, pis) = c2_trace_and_pis(&lot, 2, 8, Some(PathTamper::WrongRoot))?;
        assert!(prove_stark(&C2MerkleAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c2_wrong_path_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(2, 8, profile);
        let (trace, pis) = c2_trace_and_pis(&lot, 2, 8, Some(PathTamper::WrongPath))?;
        assert!(prove_stark(&C2MerkleAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c2_wrong_index_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(1, 8, profile);
        let (trace, pis) = c2_trace_and_pis(&lot, 1, 8, Some(PathTamper::WrongIndex))?;
        assert!(prove_stark(&C2MerkleAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_valid_nullifier_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let template = build_template(CircuitKind::C5, 8, profile)?;
        prove_and_verify(&template, &lot, 5, profile)?;
        Ok(())
    }

    #[test]
    fn c5_nonempty_old_root_accepts_a_fresh_nullifier() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let prior_lot = synthetic_lot(6, 8, profile);
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis_after_prior_nullifier(&lot, &prior_lot)?;
        prove_stark(&C5NullifierUpdateAir, trace, &pis)?;
        Ok(())
    }

    #[test]
    fn c5_existing_nullifier_cannot_be_inserted_again() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis_replay(&lot)?;
        assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_duplicate_nullifier_fails() {
        assert!(invalid_c5_duplicate(8, Profile::CoffeeSmall).is_ok());
    }

    #[test]
    fn c5_wrong_old_root_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongOldRoot))?;
        assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_wrong_new_root_index_and_state_fail() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        for tamper in [
            C5Tamper::WrongNewRoot,
            C5Tamper::WrongIndex,
            C5Tamper::WrongState,
        ] {
            let (trace, pis) = c5_trace_and_pis(&lot, Some(tamper))?;
            assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_wrong_secret_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongSecret))?;
        assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_wrong_sparse_path_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongPath))?;
        assert!(prove_stark(&C5NullifierUpdateAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_public_inputs_bind_old_root_new_root_nullifier_and_state() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (_, pis) = c5_trace_and_pis(&lot, None)?;
        let nullifier = poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0];
        assert_eq!(pis.len(), 4);
        assert_eq!(pis[2], nullifier);
        assert_eq!(pis[3], F::ONE);
        assert_ne!(pis[0], pis[1]);
        assert_ne!(pis[2], f(lot.secret));
        Ok(())
    }

    #[test]
    fn c5_template_reports_all_sparse_map_public_inputs() -> Result<()> {
        let template = build_template(CircuitKind::C5, 8, Profile::CoffeeSmall)?;
        assert_eq!(template.public_inputs, 4);
        assert!(template.note.contains("sparse"));
        Ok(())
    }

    #[test]
    fn c1_valid_outside_polygon_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(6, 8, profile);
        let template = build_template(CircuitKind::C1, 8, profile)?;
        prove_and_verify(&template, &lot, 6, profile)?;
        Ok(())
    }

    #[test]
    fn c1_inside_polygon_and_boundary_fail() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let inside = crate::synthetic::inside_forbidden_lot(6, 8, profile);
        let template = build_template(CircuitKind::C1, 8, profile)?;
        assert!(prove_and_verify(&template, &inside, 6, profile).is_err());

        let mut boundary = synthetic_lot(6, 8, profile);
        let vertex = boundary.polygon.vertices()[0];
        boundary.coords[0] = vertex;
        boundary.events[0].latitude_e6 = vertex.latitude_e6;
        boundary.events[0].longitude_e6 = vertex.longitude_e6;
        assert!(prove_and_verify(&template, &boundary, 6, profile).is_err());
        Ok(())
    }

    #[test]
    fn c1_concave_polygon_respects_the_notch() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_template(CircuitKind::C1, 8, profile)?;
        let mut lot = synthetic_lot(6, 8, profile);
        lot.polygon = SimplePolygon::new(vec![
            PointE6::new(0, 0),
            PointE6::new(10, 0),
            PointE6::new(10, 10),
            PointE6::new(5, 5),
            PointE6::new(0, 10),
        ])?;
        for (coord, event) in lot.coords.iter_mut().zip(lot.events.iter_mut()) {
            *coord = PointE6::new(5, 8);
            event.latitude_e6 = coord.latitude_e6;
            event.longitude_e6 = coord.longitude_e6;
        }
        prove_and_verify(&template, &lot, 6, profile)?;

        lot.coords[0] = PointE6::new(2, 2);
        lot.events[0].latitude_e6 = 2;
        lot.events[0].longitude_e6 = 2;
        assert!(prove_and_verify(&template, &lot, 6, profile).is_err());
        Ok(())
    }

    #[test]
    fn c1_wrong_public_commitment_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(6, 8, profile);
        for commitment in 0..2 {
            let (trace, mut pis) = c1_serial_trace_and_pis(&lot, 8)?;
            pis[commitment] += F::ONE;
            assert!(prove_stark(
                &C1SerialAir {
                    events: 8,
                    level: usize::MAX,
                },
                trace,
                &pis,
            )
            .is_err());
        }
        Ok(())
    }

    #[test]
    fn c1_constraint_degree_fits_the_fixed_fri_configuration() {
        use p3_air::symbolic::{get_max_constraint_degree, AirLayout};

        let serial = C1SerialAir {
            events: 8,
            level: usize::MAX,
        };
        let c2 = C2MerkleAir;
        let fixed_fri_limit = (1 << FRI_LOG_BLOWUP) + 1;
        assert!(
            get_max_constraint_degree::<F, _>(&serial, AirLayout::from_air(&serial))
                <= fixed_fri_limit
        );
        assert!(
            get_max_constraint_degree::<F, _>(&c2, AirLayout::from_air(&c2)) <= fixed_fri_limit
        );
    }

    #[cfg(feature = "recursion")]
    #[test]
    fn wrapper_recursive_plonky3_reports_upstream_blocker() {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(1, 8, profile);
        let err = prove_recursive_wrapper(&lot, 1, 8, profile)
            .expect_err("pinned upstream recursion rev should report blocker");
        let message = format!("{err:#}");
        assert!(message.contains("upstream Plonky3-recursion"));
        assert!(message.contains("trace_next is always present"));
    }
}
