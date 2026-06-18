use crate::backend::ProofStats;
use crate::synthetic::{poseidon2_permute, tampered_actor_lot, SyntheticLot};
use crate::types::{
    f, CircuitKind, Profile, F, POSEIDON_TAG_CERT, POSEIDON_TAG_EMPTY, POSEIDON_TAG_NULLIFIER,
    RANGE_BITS, STRICT_MERKLE_DEPTH, STRICT_MERKLE_LEAVES, THRESHOLD,
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
use p3_field::{Dup, Field, PrimeCharacteristicRing};
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
const SBOX_REGISTERS: usize = 0;
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
const C5_VECTOR_LANES: usize = 32;
const C5_INDEX_BITS_START: usize = POSEIDON_COLS * C5_VECTOR_LANES;
const C5_WIDTH: usize = C5_INDEX_BITS_START + STRICT_MERKLE_DEPTH;
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
            0,
            "blocked;not-ported:c1 geofence is outside the Plonky3 base scope for this E1 pass",
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
            2,
            "research;mvp:c5 proves Poseidon2 nullifier derivation plus hashed empty-leaf MVP non-membership; not a production accumulator",
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
        CircuitKind::C2 => prove_c2(lot, seed, template.events),
        CircuitKind::C3 => prove_c3(lot, template.events),
        CircuitKind::C4 => prove_c4(lot),
        CircuitKind::C5 => prove_c5(lot),
        CircuitKind::Wrapper => bail!(
            "Plonky3 recursion integration is only available through the wrapper runner with --features recursion"
        ),
        CircuitKind::C1 | CircuitKind::C1Legacy => {
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
        CircuitKind::C1 | CircuitKind::C1Legacy => {
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
        log_blowup: 2,
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
    let c5_air = C5NullifierEmptyAir;

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

pub struct C5NullifierEmptyAir;

impl BaseAir<F> for C5NullifierEmptyAir {
    fn width(&self) -> usize {
        C5_WIDTH
    }

    fn main_next_row_columns(&self) -> Vec<usize> {
        vec![]
    }

    fn num_public_values(&self) -> usize {
        2
    }

    fn max_constraint_degree(&self) -> Option<usize> {
        None
    }
}

impl<AB: AirBuilder<F = F>> Air<AB> for C5NullifierEmptyAir {
    fn eval(&self, builder: &mut AB) {
        let nullifier_pub = builder.public_values()[0];
        let root_pub = builder.public_values()[1];
        let main = builder.main();
        let local = main.current_slice();

        for lane in 0..C5_VECTOR_LANES {
            let cols = poseidon_lane::<AB>(local, lane);
            eval_poseidon2_cols(builder, cols);
        }

        let nullifier = poseidon_lane::<AB>(local, 0);
        builder.assert_eq(nullifier.inputs[2], f(POSEIDON_TAG_NULLIFIER));
        builder.assert_zero(nullifier.inputs[3]);
        builder.assert_zero(nullifier.inputs[4]);
        builder.assert_zero(nullifier.inputs[5]);
        builder.assert_zero(nullifier.inputs[6]);
        builder.assert_eq(nullifier.inputs[7], f(3));
        builder.assert_eq(
            nullifier.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0],
            nullifier_pub,
        );

        let empty_leaf = poseidon_lane::<AB>(local, 1);
        builder.assert_eq(empty_leaf.inputs[0], nullifier.inputs[0]);
        builder.assert_eq(empty_leaf.inputs[2], f(POSEIDON_TAG_EMPTY));
        builder.assert_zero(empty_leaf.inputs[3]);
        builder.assert_zero(empty_leaf.inputs[4]);
        builder.assert_zero(empty_leaf.inputs[5]);
        builder.assert_zero(empty_leaf.inputs[6]);
        builder.assert_eq(empty_leaf.inputs[7], f(3));

        let mut recomposed_index = AB::Expr::ZERO;
        let mut previous_output = empty_leaf.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
        for level in 0..STRICT_MERKLE_DEPTH {
            let bit = local[C5_INDEX_BITS_START + level];
            let path = poseidon_lane::<AB>(local, level + 2);
            builder.assert_zero(bit * (bit - F::ONE));
            recomposed_index += bit * f(1u64 << level);
            builder.assert_eq(path.inputs[2], f(POSEIDON_TAG_EMPTY));
            builder.assert_zero(path.inputs[3]);
            builder.assert_zero(path.inputs[4]);
            builder.assert_zero(path.inputs[5]);
            builder.assert_zero(path.inputs[6]);
            builder.assert_eq(path.inputs[7], f(3));
            builder.assert_zero((bit - F::ONE) * (path.inputs[0] - previous_output));
            builder.assert_zero(bit * (path.inputs[1] - previous_output));
            previous_output = path.ending_full_rounds[HALF_FULL_ROUNDS - 1].post[0];
        }

        builder.assert_zero(empty_leaf.inputs[1] - recomposed_index);
        builder.assert_eq(previous_output, root_pub);
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
        eval_poseidon2_sbox::<AB>(&full_round.sbox[i], s);
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
    eval_poseidon2_sbox::<AB>(&partial_round.sbox, &mut state[0]);
    builder.assert_eq(state[0].dup(), partial_round.post_sbox);
    state[0] = partial_round.post_sbox.into();
    GenericPoseidon2LinearLayersGoldilocks::internal_linear_layer(state);
}

fn eval_poseidon2_sbox<AB: AirBuilder<F = F>>(
    _sbox: &SBox<AB::Var, GOLDILOCKS_SBOX_DEGREE, SBOX_REGISTERS>,
    x: &mut AB::Expr,
) {
    *x = x.exp_const_u64::<GOLDILOCKS_SBOX_DEGREE>();
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
    let (prove_ms, verify_ms, proof_bytes) = prove_stark(&C5NullifierEmptyAir, trace, &pis)?;
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
    let witness = c5_witness(lot, tamper)?;
    Ok((
        c5_trace(witness.inputs, &witness.index_bits),
        vec![witness.nullifier, witness.root],
    ))
}

struct C5Witness {
    inputs: Vec<[F; POSEIDON_WIDTH]>,
    index_bits: Vec<F>,
    nullifier: F,
    root: F,
}

fn c5_witness(lot: &SyntheticLot, tamper: Option<C5Tamper>) -> Result<C5Witness> {
    let empty_index = (lot.lot_id as usize) & (STRICT_MERKLE_LEAVES - 1);
    let public_nullifier_input = c5_nullifier_input(lot.lot_id, lot.secret);
    let nullifier = poseidon2_permute(public_nullifier_input)[0];
    let nullifier_input = match tamper {
        Some(C5Tamper::WrongSecret) => c5_nullifier_input(lot.lot_id, lot.secret + 1),
        _ => public_nullifier_input,
    };

    let empty_leaf_input = match tamper {
        Some(C5Tamper::Duplicate) => c5_non_empty_leaf_input(lot, empty_index),
        _ => c5_empty_leaf_input(lot.lot_id, empty_index),
    };
    let empty_leaf = poseidon2_permute(c5_empty_leaf_input(lot.lot_id, empty_index))[0];
    let mut leaves = (0..STRICT_MERKLE_LEAVES)
        .map(|i| c5_leaf_hash(lot, i, empty_index))
        .collect::<Vec<_>>();

    let mut current = empty_leaf;
    let mut inputs = Vec::with_capacity(STRICT_MERKLE_DEPTH + 2);
    let mut index_bits = Vec::with_capacity(STRICT_MERKLE_DEPTH);
    inputs.push(nullifier_input);
    inputs.push(empty_leaf_input);

    let mut idx = empty_index;
    while leaves.len() > 1 {
        let bit = idx & 1;
        let sibling_idx = if bit == 0 { idx + 1 } else { idx - 1 };
        let sibling = leaves[sibling_idx];
        let parent_input = if bit == 0 {
            merkle_parent_input(current, sibling, f(POSEIDON_TAG_EMPTY))
        } else {
            merkle_parent_input(sibling, current, f(POSEIDON_TAG_EMPTY))
        };
        current = poseidon2_permute(parent_input)[0];
        inputs.push(parent_input);
        index_bits.push(f(bit as u64));
        leaves = leaves
            .chunks(2)
            .map(|pair| {
                poseidon2_permute(merkle_parent_input(pair[0], pair[1], f(POSEIDON_TAG_EMPTY)))[0]
            })
            .collect();
        idx /= 2;
    }

    let mut root = current;
    match tamper {
        Some(C5Tamper::WrongRoot) => root += F::ONE,
        Some(C5Tamper::WrongPath) => inputs[2][1] += F::ONE,
        Some(C5Tamper::WrongSecret) | Some(C5Tamper::Duplicate) => {}
        None => {}
    }
    Ok(C5Witness {
        inputs,
        index_bits,
        nullifier,
        root,
    })
}

fn c5_trace(inputs: Vec<[F; POSEIDON_WIDTH]>, index_bits: &[F]) -> RowMajorMatrix<F> {
    debug_assert_eq!(inputs.len(), STRICT_MERKLE_DEPTH + 2);
    debug_assert_eq!(index_bits.len(), STRICT_MERKLE_DEPTH);
    let poseidon = poseidon_trace(inputs);
    debug_assert_eq!(poseidon.values.len(), POSEIDON_COLS * C5_VECTOR_LANES);
    let mut values = Vec::with_capacity(C5_WIDTH);
    values.extend_from_slice(&poseidon.values);
    values.extend_from_slice(index_bits);
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

fn c5_empty_leaf_input(lot_id: u64, index: usize) -> [F; POSEIDON_WIDTH] {
    [
        f(lot_id),
        f(index as u64),
        f(POSEIDON_TAG_EMPTY),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(3),
    ]
}

fn c5_non_empty_leaf_input(lot: &SyntheticLot, index: usize) -> [F; POSEIDON_WIDTH] {
    [
        f(lot.lot_id + 41 + index as u64),
        f(lot.secret + 7 + index as u64),
        f(POSEIDON_TAG_NULLIFIER),
        F::ZERO,
        F::ZERO,
        F::ZERO,
        F::ZERO,
        f(3),
    ]
}

fn c5_leaf_hash(lot: &SyntheticLot, index: usize, empty_index: usize) -> F {
    let input = if index == empty_index {
        c5_empty_leaf_input(lot.lot_id, index)
    } else {
        c5_non_empty_leaf_input(lot, index)
    };
    poseidon2_permute(input)[0]
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum C5Tamper {
    Duplicate,
    WrongRoot,
    WrongPath,
    WrongSecret,
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
    assert!(prove_stark(&C5NullifierEmptyAir, trace, &pis).is_err());
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
    fn c5_duplicate_nullifier_fails() {
        assert!(invalid_c5_duplicate(8, Profile::CoffeeSmall).is_ok());
    }

    #[test]
    fn c5_wrong_empty_root_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongRoot))?;
        assert!(prove_stark(&C5NullifierEmptyAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_wrong_secret_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongSecret))?;
        assert!(prove_stark(&C5NullifierEmptyAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_wrong_empty_path_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (trace, pis) = c5_trace_and_pis(&lot, Some(C5Tamper::WrongPath))?;
        assert!(prove_stark(&C5NullifierEmptyAir, trace, &pis).is_err());
        Ok(())
    }

    #[test]
    fn c5_public_inputs_are_nullifier_and_empty_root_only() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let lot = synthetic_lot(5, 8, profile);
        let (_, pis) = c5_trace_and_pis(&lot, None)?;
        let nullifier = poseidon2_permute(c5_nullifier_input(lot.lot_id, lot.secret))[0];
        assert_eq!(pis.len(), 2);
        assert_eq!(pis[0], nullifier);
        assert_ne!(pis[0], f(lot.secret));
        assert_ne!(pis[1], f(lot.secret));
        Ok(())
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
