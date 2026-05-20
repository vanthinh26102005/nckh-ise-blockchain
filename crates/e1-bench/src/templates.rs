use crate::synthetic::{poseidon_hash, tampered_signature_lot, SyntheticLot};
use crate::types::{
    f, CircuitKind, Profile, C, D, F, POSEIDON_TAG_CERT, POSEIDON_TAG_NULLIFIER,
    POSEIDON_TAG_POLYGON, RANGE_BITS, SCHNORR_G, THRESHOLD,
};
use anyhow::{bail, Result};
use plonky2::field::types::Field;
use plonky2::hash::hash_types::{HashOut, HashOutTarget};
use plonky2::hash::merkle_proofs::MerkleProofTarget;
use plonky2::hash::merkle_tree::MerkleTree;
use plonky2::hash::poseidon::PoseidonHash;
use plonky2::iop::target::{BoolTarget, Target};
use plonky2::iop::witness::{PartialWitness, WitnessWrite};
use plonky2::plonk::circuit_builder::CircuitBuilder;
use plonky2::plonk::circuit_data::{CircuitConfig, CircuitData};
use plonky2::plonk::config::Hasher;
use plonky2::plonk::proof::{ProofWithPublicInputs, ProofWithPublicInputsTarget};
use std::collections::HashMap;
use std::time::Instant;

pub struct TemplateCache {
    templates: HashMap<CircuitKind, CircuitTemplate>,
}

impl TemplateCache {
    pub fn build(events: usize, profile: Profile, circuits: &[CircuitKind]) -> Result<Self> {
        let mut templates = HashMap::new();
        let needs_wrapper = circuits.contains(&CircuitKind::Wrapper);
        let base_kinds = [
            CircuitKind::C1,
            CircuitKind::C2,
            CircuitKind::C3,
            CircuitKind::C4,
            CircuitKind::C5,
        ];

        for kind in base_kinds {
            if needs_wrapper || circuits.contains(&kind) {
                templates.insert(kind, build_template(kind, events, profile)?);
            }
        }

        for &kind in circuits {
            if !templates.contains_key(&kind) && kind != CircuitKind::Wrapper {
                templates.insert(kind, build_template(kind, events, profile)?);
            }
        }

        if needs_wrapper {
            let inner = base_kinds
                .iter()
                .map(|kind| templates.get(kind).expect("inner template exists"))
                .collect::<Vec<_>>();
            templates.insert(
                CircuitKind::Wrapper,
                build_wrapper_template(events, profile, &inner)?,
            );
        }

        Ok(Self { templates })
    }

    pub fn get(&self, kind: CircuitKind) -> &CircuitTemplate {
        self.templates.get(&kind).expect("template built")
    }
}

pub struct CircuitTemplate {
    pub kind: CircuitKind,
    pub data: CircuitData<F, C, D>,
    pub targets: TemplateTargets,
    pub build_ms: f64,
    pub gate_count: usize,
    pub public_inputs: usize,
    pub note: &'static str,
}

pub enum TemplateTargets {
    C1(C1Targets),
    C2(C2Targets),
    C3(C3Targets),
    C4(C4Targets),
    C5(C5Targets),
    Wrapper(WrapperTargets),
    LegacyC1(LegacyC1Targets),
    LegacyC4(LegacyC4Targets),
}

pub struct C1Targets {
    points: Vec<(Target, Target)>,
    vertices: Vec<(Target, Target)>,
    commitment: HashOutTarget,
}

pub struct C2Targets {
    cert: Target,
    root: HashOutTarget,
    index_bits: Vec<BoolTarget>,
    proof: MerkleProofTarget,
}

pub struct C3Targets {
    readings: Vec<Target>,
    timestamps: Vec<Target>,
}

pub struct C4Targets {
    sigs: Vec<SchnorrTargets>,
}

pub struct SchnorrTargets {
    msg: Target,
    pk: Target,
    r_point: Target,
    s: Target,
}

pub struct C5Targets {
    lot_id: Target,
    epoch: Target,
    nullifier: HashOutTarget,
    previous: Vec<HashOutTarget>,
    previous_root: HashOutTarget,
    inverses: Vec<Target>,
}

pub struct WrapperTargets {
    proofs: Vec<ProofWithPublicInputsTarget<D>>,
}

pub struct LegacyC1Targets {
    points: Vec<(Target, Target)>,
}

pub struct LegacyC4Targets {
    rows: Vec<(Target, Target, Target)>,
}

pub struct WitnessBundle {
    pub witness: PartialWitness<F>,
    pub witness_ms: f64,
}

pub struct InnerProofBundle {
    pub proof: ProofWithPublicInputs<F, C, D>,
    pub prove_ms: f64,
}

pub fn build_template(
    kind: CircuitKind,
    events: usize,
    profile: Profile,
) -> Result<CircuitTemplate> {
    match kind {
        CircuitKind::C1 => build_c1_polygon(events, profile),
        CircuitKind::C2 => build_c2_poseidon_merkle(events),
        CircuitKind::C3 => build_c3_threshold_time(events),
        CircuitKind::C4 => build_c4_schnorr(events),
        CircuitKind::C5 => build_c5_nullifier(events, profile),
        CircuitKind::C1Legacy => build_c1_legacy(events),
        CircuitKind::C4Legacy => build_c4_legacy(events),
        CircuitKind::Wrapper => bail!("wrapper template requires inner templates"),
    }
}

pub fn witness_for(
    template: &CircuitTemplate,
    lot: &SyntheticLot,
    seed: usize,
    profile: Profile,
) -> Result<WitnessBundle> {
    let start = Instant::now();
    let mut pw = PartialWitness::new();
    match &template.targets {
        TemplateTargets::C1(targets) => set_c1_witness(&mut pw, targets, lot)?,
        TemplateTargets::C2(targets) => set_c2_witness(&mut pw, targets, lot, seed)?,
        TemplateTargets::C3(targets) => set_c3_witness(&mut pw, targets, lot)?,
        TemplateTargets::C4(targets) => set_c4_witness(&mut pw, targets, lot)?,
        TemplateTargets::C5(targets) => set_c5_witness(&mut pw, targets, lot, profile, false)?,
        TemplateTargets::LegacyC1(targets) => set_legacy_c1_witness(&mut pw, targets, lot)?,
        TemplateTargets::LegacyC4(targets) => set_legacy_c4_witness(&mut pw, targets, lot)?,
        TemplateTargets::Wrapper(_) => bail!("wrapper witness requires inner proofs"),
    }
    Ok(WitnessBundle {
        witness: pw,
        witness_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

pub fn wrapper_witness(
    template: &CircuitTemplate,
    inner_proofs: &[ProofWithPublicInputs<F, C, D>],
) -> Result<WitnessBundle> {
    let start = Instant::now();
    let mut pw = PartialWitness::new();
    let TemplateTargets::Wrapper(targets) = &template.targets else {
        bail!("not a wrapper template");
    };
    for (target, proof) in targets.proofs.iter().zip(inner_proofs) {
        pw.set_proof_with_pis_target(target, proof)?;
    }
    Ok(WitnessBundle {
        witness: pw,
        witness_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

pub fn prove_inner(
    template: &CircuitTemplate,
    lot: &SyntheticLot,
    seed: usize,
    profile: Profile,
) -> Result<InnerProofBundle> {
    let witness = witness_for(template, lot, seed, profile)?;
    let start = Instant::now();
    let proof = template.data.prove(witness.witness)?;
    let prove_ms = start.elapsed().as_secs_f64() * 1000.0;
    template.data.verify(proof.clone())?;
    Ok(InnerProofBundle { proof, prove_ms })
}

fn finish_template(
    kind: CircuitKind,
    builder: CircuitBuilder<F, D>,
    targets: TemplateTargets,
    start: Instant,
    note: &'static str,
) -> CircuitTemplate {
    let gate_count = builder.num_gates();
    let public_inputs = builder.num_public_inputs();
    let data = builder.build::<C>();
    CircuitTemplate {
        kind,
        data,
        targets,
        build_ms: start.elapsed().as_secs_f64() * 1000.0,
        gate_count,
        public_inputs,
        note,
    }
}

fn new_builder() -> CircuitBuilder<F, D> {
    CircuitBuilder::<F, D>::new(CircuitConfig::standard_recursion_config())
}

fn build_c1_polygon(events: usize, profile: Profile) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let mut points = Vec::with_capacity(events);
    let mut vertices = Vec::with_capacity(profile.polygon().len());
    let mut commitment_inputs = vec![builder.constant(f(POSEIDON_TAG_POLYGON))];

    for _ in profile.polygon() {
        let x = builder.add_virtual_target();
        let y = builder.add_virtual_target();
        commitment_inputs.extend([x, y]);
        vertices.push((x, y));
    }

    let commitment_calc = builder.hash_n_to_hash_no_pad::<PoseidonHash>(commitment_inputs);
    let commitment = builder.add_virtual_hash_public_input();
    builder.connect_hashes(commitment_calc, commitment);

    for _ in 0..events {
        let x = builder.add_virtual_target();
        let y = builder.add_virtual_target();
        points.push((x, y));
        for i in 0..vertices.len() {
            let (x1, y1) = vertices[i];
            let (x2, y2) = vertices[(i + 1) % vertices.len()];
            let slack = orientation_slack_target(&mut builder, x, y, x1, y1, x2, y2);
            builder.range_check(slack, RANGE_BITS);
        }
    }

    Ok(finish_template(
        CircuitKind::C1,
        builder,
        TemplateTargets::C1(C1Targets {
            points,
            vertices,
            commitment,
        }),
        start,
        "research;real:c1 proves fixed-point point-in-convex-polygon by cross-product orientation; polygon vertices are private and commitment is public",
    ))
}

fn build_c2_poseidon_merkle(events: usize) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let cert = builder.add_virtual_target();
    let index_bits = (0..events.trailing_zeros() as usize)
        .map(|_| builder.add_virtual_bool_target_safe())
        .collect::<Vec<_>>();
    let root = builder.add_virtual_hash_public_input();
    let proof = virtual_merkle_proof(&mut builder, index_bits.len());
    let tag = builder.constant(f(POSEIDON_TAG_CERT));
    builder.verify_merkle_proof::<PoseidonHash>(vec![cert, tag], &index_bits, root, &proof);

    Ok(finish_template(
        CircuitKind::C2,
        builder,
        TemplateTargets::C2(C2Targets {
            cert,
            root,
            index_bits,
            proof,
        }),
        start,
        "research;real:c2 verifies private certificate membership against a Poseidon Merkle root",
    ))
}

fn build_c3_threshold_time(events: usize) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let threshold = builder.constant(f(THRESHOLD));
    let mut readings = Vec::with_capacity(events);
    let mut timestamps = Vec::with_capacity(events);
    let mut acc_inputs = vec![threshold];

    for i in 0..events {
        let reading = builder.add_virtual_target();
        let timestamp = builder.add_virtual_target();
        readings.push(reading);
        timestamps.push(timestamp);
        builder.range_check(reading, RANGE_BITS);
        let diff = builder.sub(threshold, reading);
        builder.range_check(diff, RANGE_BITS);
        if i > 0 {
            let monotonic = builder.sub(timestamp, timestamps[i - 1]);
            builder.range_check(monotonic, RANGE_BITS);
        }
        acc_inputs.push(reading);
        acc_inputs.push(timestamp);
    }

    let commitment = builder.hash_n_to_hash_no_pad::<PoseidonHash>(acc_inputs);
    builder.register_public_input(threshold);
    builder.register_public_inputs(&commitment.elements);

    Ok(finish_template(
        CircuitKind::C3,
        builder,
        TemplateTargets::C3(C3Targets {
            readings,
            timestamps,
        }),
        start,
        "research;real:c3 proves readings stay under threshold and EPCIS event time is monotonic",
    ))
}

fn build_c4_schnorr(events: usize) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let g = builder.constant(f(SCHNORR_G));
    let mut sigs = Vec::with_capacity(events);
    let mut commitment_inputs = Vec::with_capacity(events * 3);

    for _ in 0..events {
        let msg = builder.add_virtual_target();
        let pk = builder.add_virtual_target();
        let r_point = builder.add_virtual_target();
        let s = builder.add_virtual_target();
        let challenge_hash = builder.hash_n_to_hash_no_pad::<PoseidonHash>(vec![r_point, pk, msg]);
        let challenge = challenge_hash.elements[0];
        let lhs = builder.mul(s, g);
        let e_pk = builder.mul(challenge, pk);
        let rhs = builder.add(r_point, e_pk);
        builder.connect(lhs, rhs);
        commitment_inputs.extend([msg, pk, r_point]);
        sigs.push(SchnorrTargets {
            msg,
            pk,
            r_point,
            s,
        });
    }

    let statement_commitment = builder.hash_n_to_hash_no_pad::<PoseidonHash>(commitment_inputs);
    builder.register_public_inputs(&statement_commitment.elements);

    Ok(finish_template(
        CircuitKind::C4,
        builder,
        TemplateTargets::C4(C4Targets { sigs }),
        start,
        "research;proxy:c4 verifies a Schnorr-like algebraic signature over the field; not EdDSA production",
    ))
}

fn build_c5_nullifier(events: usize, profile: Profile) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let previous_count = profile.previous_nullifier_count(events);
    let mut builder = new_builder();
    let lot_id = builder.add_virtual_target();
    let epoch = builder.add_virtual_target();
    let tag = builder.constant(f(POSEIDON_TAG_NULLIFIER));
    let nullifier_calc = builder.hash_n_to_hash_no_pad::<PoseidonHash>(vec![lot_id, epoch, tag]);
    let nullifier = builder.add_virtual_hash_public_input();
    builder.connect_hashes(nullifier_calc, nullifier);

    let mut previous = Vec::with_capacity(previous_count);
    let mut inverses = Vec::with_capacity(previous_count);
    for _ in 0..previous_count {
        let prev = builder.add_virtual_hash();
        let inv = builder.add_virtual_target();
        let diff = builder.sub(nullifier.elements[0], prev.elements[0]);
        let product = builder.mul(diff, inv);
        builder.assert_one(product);
        previous.push(prev);
        inverses.push(inv);
    }

    let previous_root_calc = merkle_root_targets(&mut builder, previous.clone());
    let previous_root = builder.add_virtual_hash_public_input();
    builder.connect_hashes(previous_root_calc, previous_root);

    Ok(finish_template(
        CircuitKind::C5,
        builder,
        TemplateTargets::C5(C5Targets {
            lot_id,
            epoch,
            nullifier,
            previous,
            previous_root,
            inverses,
        }),
        start,
        "research;real:c5 proves Poseidon nullifier derivation and non-membership against a committed bounded nullifier set",
    ))
}

fn build_wrapper_template(
    events: usize,
    profile: Profile,
    inner: &[&CircuitTemplate],
) -> Result<CircuitTemplate> {
    let _ = (events, profile);
    let start = Instant::now();
    let mut builder = new_builder();
    let mut proof_targets = Vec::with_capacity(inner.len());
    let mut aggregate_inputs = Vec::new();

    for template in inner {
        let pt = builder.add_virtual_proof_with_pis(&template.data.common);
        let verifier_data = builder.constant_verifier_data::<C>(&template.data.verifier_only);
        builder.verify_proof::<C>(&pt, &verifier_data, &template.data.common);
        aggregate_inputs.extend(pt.public_inputs.iter().copied());
        proof_targets.push(pt);
    }

    let aggregate = builder.hash_n_to_hash_no_pad::<PoseidonHash>(aggregate_inputs);
    builder.register_public_inputs(&aggregate.elements);

    Ok(finish_template(
        CircuitKind::Wrapper,
        builder,
        TemplateTargets::Wrapper(WrapperTargets {
            proofs: proof_targets,
        }),
        start,
        "research;real:wrapper recursively verifies C1-C5 Plonky2 proofs in one outer proof",
    ))
}

fn build_c1_legacy(events: usize) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let mut points = Vec::with_capacity(events);
    let min_x = builder.constant(f(900));
    let max_x = builder.constant(f(1600));
    let min_y = builder.constant(f(1900));
    let max_y = builder.constant(f(2600));
    for _ in 0..events {
        let x = builder.add_virtual_target();
        let y = builder.add_virtual_target();
        let x_min = builder.sub(x, min_x);
        let max_x_diff = builder.sub(max_x, x);
        let y_min = builder.sub(y, min_y);
        let max_y_diff = builder.sub(max_y, y);
        builder.range_check(x_min, RANGE_BITS);
        builder.range_check(max_x_diff, RANGE_BITS);
        builder.range_check(y_min, RANGE_BITS);
        builder.range_check(max_y_diff, RANGE_BITS);
        points.push((x, y));
    }
    Ok(finish_template(
        CircuitKind::C1Legacy,
        builder,
        TemplateTargets::LegacyC1(LegacyC1Targets { points }),
        start,
        "compat;placeholder:c1 legacy bounding-box circuit retained only for comparison",
    ))
}

fn build_c4_legacy(events: usize) -> Result<CircuitTemplate> {
    let start = Instant::now();
    let mut builder = new_builder();
    let mut rows = Vec::with_capacity(events);
    for _ in 0..events {
        let msg = builder.add_virtual_target();
        let signer = builder.add_virtual_target();
        let sig = builder.add_virtual_target();
        let expected = builder.hash_n_to_hash_no_pad::<PoseidonHash>(vec![msg, signer]);
        builder.connect(sig, expected.elements[0]);
        rows.push((msg, signer, sig));
    }
    Ok(finish_template(
        CircuitKind::C4Legacy,
        builder,
        TemplateTargets::LegacyC4(LegacyC4Targets { rows }),
        start,
        "compat;placeholder:c4 legacy commitment check retained only for comparison",
    ))
}

fn set_c1_witness(
    pw: &mut PartialWitness<F>,
    targets: &C1Targets,
    lot: &SyntheticLot,
) -> Result<()> {
    let commitment = polygon_commitment(&lot.polygon);
    pw.set_hash_target(targets.commitment, commitment)?;
    for ((x_t, y_t), &(x, y)) in targets.vertices.iter().zip(&lot.polygon) {
        pw.set_target(*x_t, f(x))?;
        pw.set_target(*y_t, f(y))?;
    }
    for ((x_t, y_t), &(x, y)) in targets.points.iter().zip(&lot.coords) {
        pw.set_target(*x_t, f(x))?;
        pw.set_target(*y_t, f(y))?;
    }
    Ok(())
}

fn set_c2_witness(
    pw: &mut PartialWitness<F>,
    targets: &C2Targets,
    lot: &SyntheticLot,
    seed: usize,
) -> Result<()> {
    let index = seed % lot.cert_ids.len();
    let (tree, root) = cert_tree(lot);
    let proof = tree.prove(index);
    pw.set_target(targets.cert, f(lot.cert_ids[index]))?;
    pw.set_hash_target(targets.root, root)?;
    for (bit_target, bit) in targets
        .index_bits
        .iter()
        .zip(index_bits(index, targets.index_bits.len()))
    {
        pw.set_bool_target(*bit_target, bit)?;
    }
    for (target, sibling) in targets.proof.siblings.iter().zip(proof.siblings) {
        pw.set_hash_target(*target, sibling)?;
    }
    Ok(())
}

fn set_c3_witness(
    pw: &mut PartialWitness<F>,
    targets: &C3Targets,
    lot: &SyntheticLot,
) -> Result<()> {
    for ((reading_t, timestamp_t), (&reading, &timestamp)) in targets
        .readings
        .iter()
        .zip(&targets.timestamps)
        .zip(lot.readings.iter().zip(&lot.timestamps))
    {
        pw.set_target(*reading_t, f(reading))?;
        pw.set_target(*timestamp_t, f(timestamp))?;
    }
    Ok(())
}

fn set_c4_witness(
    pw: &mut PartialWitness<F>,
    targets: &C4Targets,
    lot: &SyntheticLot,
) -> Result<()> {
    for (target, sig) in targets.sigs.iter().zip(&lot.signatures) {
        pw.set_target(target.msg, f(sig.msg))?;
        pw.set_target(target.pk, sig.pk)?;
        pw.set_target(target.r_point, sig.r_point)?;
        pw.set_target(target.s, sig.s)?;
    }
    Ok(())
}

fn set_c5_witness(
    pw: &mut PartialWitness<F>,
    targets: &C5Targets,
    lot: &SyntheticLot,
    profile: Profile,
    force_duplicate: bool,
) -> Result<()> {
    let nullifier = nullifier_hash(lot.lot_id, lot.epoch);
    let previous = previous_nullifiers(lot, profile, force_duplicate);
    let previous_root = merkle_root_values(&previous);
    pw.set_target(targets.lot_id, f(lot.lot_id))?;
    pw.set_target(targets.epoch, f(lot.epoch))?;
    pw.set_hash_target(targets.nullifier, nullifier)?;
    pw.set_hash_target(targets.previous_root, previous_root)?;
    for ((target, inv_target), prev) in targets.previous.iter().zip(&targets.inverses).zip(previous)
    {
        if prev.elements[0] == nullifier.elements[0] {
            bail!("generated duplicate nullifier for non-membership test");
        }
        pw.set_hash_target(*target, prev)?;
        pw.set_target(
            *inv_target,
            (nullifier.elements[0] - prev.elements[0]).inverse(),
        )?;
    }
    Ok(())
}

fn set_legacy_c1_witness(
    pw: &mut PartialWitness<F>,
    targets: &LegacyC1Targets,
    lot: &SyntheticLot,
) -> Result<()> {
    for ((x_t, y_t), &(x, y)) in targets.points.iter().zip(&lot.coords) {
        pw.set_target(*x_t, f(x))?;
        pw.set_target(*y_t, f(y))?;
    }
    Ok(())
}

fn set_legacy_c4_witness(
    pw: &mut PartialWitness<F>,
    targets: &LegacyC4Targets,
    lot: &SyntheticLot,
) -> Result<()> {
    for (i, (msg_t, signer_t, sig_t)) in targets.rows.iter().enumerate() {
        let msg = f(lot.cert_ids[i] + lot.readings[i]);
        let signer = f(10_000 + i as u64);
        let sig = poseidon_hash(&[msg, signer]).elements[0];
        pw.set_target(*msg_t, msg)?;
        pw.set_target(*signer_t, signer)?;
        pw.set_target(*sig_t, sig)?;
    }
    Ok(())
}

pub fn invalid_c1_outside(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, PartialWitness<F>)> {
    let template = build_c1_polygon(events, profile)?;
    let lot = crate::synthetic::outside_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?.witness;
    Ok((template, witness))
}

pub fn invalid_c3_overflow(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, PartialWitness<F>)> {
    let template = build_c3_threshold_time(events)?;
    let lot = crate::synthetic::threshold_overflow_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?.witness;
    Ok((template, witness))
}

pub fn invalid_c4_tampered(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, PartialWitness<F>)> {
    let template = build_c4_schnorr(events)?;
    let lot = tampered_signature_lot(3, events, profile);
    let witness = witness_for(&template, &lot, 3, profile)?.witness;
    Ok((template, witness))
}

pub fn invalid_c5_duplicate(
    events: usize,
    profile: Profile,
) -> Result<(CircuitTemplate, PartialWitness<F>)> {
    let template = build_c5_nullifier(events, profile)?;
    let lot = crate::synthetic::synthetic_lot(3, events, profile);
    let mut pw = PartialWitness::new();
    let TemplateTargets::C5(targets) = &template.targets else {
        unreachable!();
    };
    set_c5_witness(&mut pw, targets, &lot, profile, true)?;
    Ok((template, pw))
}

fn virtual_merkle_proof(builder: &mut CircuitBuilder<F, D>, len: usize) -> MerkleProofTarget {
    MerkleProofTarget {
        siblings: (0..len).map(|_| builder.add_virtual_hash()).collect(),
    }
}

fn cert_tree(lot: &SyntheticLot) -> (MerkleTree<F, PoseidonHash>, HashOut<F>) {
    let leaves = lot
        .cert_ids
        .iter()
        .map(|&id| vec![f(id), f(POSEIDON_TAG_CERT)])
        .collect::<Vec<_>>();
    let tree = MerkleTree::<F, PoseidonHash>::new(leaves, 0);
    let root = tree.cap.0[0];
    (tree, root)
}

fn index_bits(index: usize, bits: usize) -> Vec<bool> {
    (0..bits).map(|i| ((index >> i) & 1) == 1).collect()
}

fn polygon_commitment(polygon: &[(u64, u64)]) -> HashOut<F> {
    let mut inputs = vec![f(POSEIDON_TAG_POLYGON)];
    for &(x, y) in polygon {
        inputs.push(f(x));
        inputs.push(f(y));
    }
    poseidon_hash(&inputs)
}

fn orientation_slack_target(
    builder: &mut CircuitBuilder<F, D>,
    px: Target,
    py: Target,
    x1: Target,
    y1: Target,
    x2: Target,
    y2: Target,
) -> Target {
    let dx = builder.sub(x2, x1);
    let dy = builder.sub(y2, y1);
    let py_y1 = builder.sub(py, y1);
    let px_x1 = builder.sub(px, x1);
    let left = builder.mul(dx, py_y1);
    let right = builder.mul(dy, px_x1);
    builder.sub(left, right)
}

fn nullifier_hash(lot_id: u64, epoch: u64) -> HashOut<F> {
    poseidon_hash(&[f(lot_id), f(epoch), f(POSEIDON_TAG_NULLIFIER)])
}

fn previous_nullifiers(
    lot: &SyntheticLot,
    profile: Profile,
    force_duplicate: bool,
) -> Vec<HashOut<F>> {
    let count = profile.previous_nullifier_count(lot.readings.len());
    let duplicate = nullifier_hash(lot.lot_id, lot.epoch);
    (0..count)
        .map(|i| {
            if force_duplicate && i == count / 2 {
                duplicate
            } else {
                poseidon_hash(&[
                    f(lot.lot_id + 41 + i as u64),
                    f(lot.epoch + 7 + i as u64),
                    f(POSEIDON_TAG_NULLIFIER),
                ])
            }
        })
        .collect()
}

fn merkle_root_values(values: &[HashOut<F>]) -> HashOut<F> {
    let mut level = values.to_vec();
    while level.len() > 1 {
        level = level
            .chunks_exact(2)
            .map(|pair| PoseidonHash::two_to_one(pair[0], pair[1]))
            .collect();
    }
    level[0]
}

fn merkle_root_targets(
    builder: &mut CircuitBuilder<F, D>,
    values: Vec<HashOutTarget>,
) -> HashOutTarget {
    let mut level = values;
    while level.len() > 1 {
        level = level
            .chunks_exact(2)
            .map(|pair| two_to_one_target(builder, pair[0], pair[1]))
            .collect();
    }
    level[0]
}

fn two_to_one_target(
    builder: &mut CircuitBuilder<F, D>,
    left: HashOutTarget,
    right: HashOutTarget,
) -> HashOutTarget {
    let mut inputs = left.elements.to_vec();
    inputs.extend(right.elements);
    builder.hash_n_to_hash_no_pad::<PoseidonHash>(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::synthetic_lot;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn proves(
        template: &CircuitTemplate,
        lot: &SyntheticLot,
        seed: usize,
        profile: Profile,
    ) -> Result<()> {
        let witness = witness_for(template, lot, seed, profile)?;
        let proof = template.data.prove(witness.witness)?;
        template.data.verify(proof)?;
        Ok(())
    }

    #[test]
    fn c1_inside_polygon_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c1_polygon(8, profile)?;
        let lot = synthetic_lot(1, 8, profile);
        proves(&template, &lot, 1, profile)
    }

    #[test]
    fn c1_outside_polygon_fails() -> Result<()> {
        let (template, witness) = invalid_c1_outside(8, Profile::CoffeeSmall)?;
        assert_prove_fails(&template, witness);
        Ok(())
    }

    #[test]
    fn c2_valid_merkle_path_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c2_poseidon_merkle(8)?;
        let lot = synthetic_lot(2, 8, profile);
        proves(&template, &lot, 2, profile)
    }

    #[test]
    fn c2_wrong_root_fails() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c2_poseidon_merkle(8)?;
        let lot = synthetic_lot(2, 8, profile);
        let TemplateTargets::C2(targets) = &template.targets else {
            unreachable!();
        };
        let mut pw = PartialWitness::new();
        set_c2_witness(&mut pw, targets, &lot, 2)?;
        assert!(pw.set_hash_target(targets.root, HashOut::ZERO).is_err());
        Ok(())
    }

    #[test]
    fn c3_valid_threshold_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c3_threshold_time(8)?;
        let lot = synthetic_lot(3, 8, profile);
        proves(&template, &lot, 3, profile)
    }

    #[test]
    fn c3_threshold_overflow_fails() -> Result<()> {
        let (template, witness) = invalid_c3_overflow(8, Profile::CoffeeSmall)?;
        assert_prove_fails(&template, witness);
        Ok(())
    }

    #[test]
    fn c4_valid_schnorr_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c4_schnorr(8)?;
        let lot = synthetic_lot(4, 8, profile);
        proves(&template, &lot, 4, profile)
    }

    #[test]
    fn c4_tampered_signature_fails() -> Result<()> {
        let (template, witness) = invalid_c4_tampered(8, Profile::CoffeeSmall)?;
        assert_prove_fails(&template, witness);
        Ok(())
    }

    #[test]
    fn c5_duplicate_nullifier_fails() {
        assert!(invalid_c5_duplicate(8, Profile::CoffeeSmall).is_err());
    }

    #[test]
    fn c5_valid_nullifier_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let template = build_c5_nullifier(8, profile)?;
        let lot = synthetic_lot(5, 8, profile);
        proves(&template, &lot, 5, profile)
    }

    #[test]
    fn wrapper_recursive_proof_passes() -> Result<()> {
        let profile = Profile::CoffeeSmall;
        let events = 8;
        let lot = synthetic_lot(6, events, profile);
        let c1 = build_c1_polygon(events, profile)?;
        let c2 = build_c2_poseidon_merkle(events)?;
        let c3 = build_c3_threshold_time(events)?;
        let c4 = build_c4_schnorr(events)?;
        let c5 = build_c5_nullifier(events, profile)?;
        let inner_templates = [&c1, &c2, &c3, &c4, &c5];
        let wrapper = build_wrapper_template(events, profile, &inner_templates)?;
        let inner_proofs = inner_templates
            .iter()
            .map(|template| prove_inner(template, &lot, 6, profile).map(|bundle| bundle.proof))
            .collect::<Result<Vec<_>>>()?;
        let witness = wrapper_witness(&wrapper, &inner_proofs)?;
        let proof = wrapper.data.prove(witness.witness)?;
        wrapper.data.verify(proof)?;
        Ok(())
    }

    fn assert_prove_fails(template: &CircuitTemplate, witness: PartialWitness<F>) {
        let result = catch_unwind(AssertUnwindSafe(|| template.data.prove(witness)));
        assert!(result.is_err() || result.unwrap().is_err());
    }
}
