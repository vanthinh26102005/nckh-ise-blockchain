use crate::types::{f, Profile, F, SCHNORR_G, THRESHOLD};
use plonky2::hash::hash_types::HashOut;
use plonky2::hash::hashing::hash_n_to_hash_no_pad;
use plonky2::hash::poseidon::PoseidonPermutation;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticLot {
    pub readings: Vec<u64>,
    pub timestamps: Vec<u64>,
    pub coords: Vec<(u64, u64)>,
    pub cert_ids: Vec<u64>,
    pub lot_id: u64,
    pub epoch: u64,
    pub polygon: Vec<(u64, u64)>,
    pub signatures: Vec<ToySignature>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToySignature {
    pub msg: u64,
    pub sk: F,
    pub pk: F,
    pub r_point: F,
    pub s: F,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HalfPlane {
    pub a: i64,
    pub b: i64,
    pub c: i64,
}

pub fn synthetic_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ ((events as u64) << 32) ^ profile_tag(profile));
    let polygon = profile.polygon();
    let readings: Vec<u64> = (0..events)
        .map(|_| rng.gen_range(100..=THRESHOLD))
        .collect();
    let timestamps: Vec<u64> = (0..events)
        .map(|i| 1_700_000_000 + seed * 1000 + i as u64 * 15)
        .collect();
    let coords = inside_points(&mut rng, events, profile);
    let cert_ids: Vec<u64> = (0..events)
        .map(|_| rng.gen_range(10_000..=999_999))
        .collect();
    let lot_id = rng.gen_range(1_000_000..=9_999_999);
    let epoch = 1_800_000_000 + seed;
    let signatures = cert_ids
        .iter()
        .zip(&readings)
        .enumerate()
        .map(|(i, (&cert, &reading))| toy_sign(seed, i, cert + reading))
        .collect();

    SyntheticLot {
        readings,
        timestamps,
        coords,
        cert_ids,
        lot_id,
        epoch,
        polygon,
        signatures,
    }
}

fn profile_tag(profile: Profile) -> u64 {
    match profile {
        Profile::CoffeeSmall => 0xC0FFEE01,
        Profile::CoffeeDefault => 0xC0FFEE02,
        Profile::Stress => 0xC0FFEE03,
    }
}

fn inside_points(rng: &mut ChaCha20Rng, events: usize, profile: Profile) -> Vec<(u64, u64)> {
    let (x0, x1, y0, y1) = match profile {
        Profile::CoffeeSmall => (1040, 1390, 2180, 2450),
        Profile::CoffeeDefault => (1030, 1450, 2180, 2520),
        Profile::Stress => (1010, 1490, 2160, 2530),
    };
    (0..events)
        .map(|_| (rng.gen_range(x0..=x1), rng.gen_range(y0..=y1)))
        .collect()
}

pub fn outside_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if let Some(first) = lot.coords.first_mut() {
        *first = (300, 300);
    }
    lot
}

pub fn threshold_overflow_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if let Some(first) = lot.readings.first_mut() {
        *first = THRESHOLD + 1;
    }
    lot
}

pub fn tampered_signature_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if let Some(first) = lot.signatures.first_mut() {
        first.s += f(1);
    }
    lot
}

pub fn halfplanes_for_polygon(poly: &[(u64, u64)]) -> Vec<HalfPlane> {
    let mut planes = Vec::with_capacity(poly.len());
    for i in 0..poly.len() {
        let (x1, y1) = poly[i];
        let (x2, y2) = poly[(i + 1) % poly.len()];
        let dx = x2 as i64 - x1 as i64;
        let dy = y2 as i64 - y1 as i64;
        let a = -(dy);
        let b = dx;
        let c = dy * x1 as i64 - dx * y1 as i64;
        planes.push(HalfPlane { a, b, c });
    }
    planes
}

pub fn poseidon_hash(inputs: &[F]) -> HashOut<F> {
    hash_n_to_hash_no_pad::<F, PoseidonPermutation<F>>(inputs)
}

pub fn toy_challenge(r_point: F, pk: F, msg: F) -> F {
    poseidon_hash(&[r_point, pk, msg]).elements[0]
}

fn toy_sign(seed: u64, event_index: usize, msg: u64) -> ToySignature {
    let sk = f(100_000 + seed * 97 + event_index as u64 * 13);
    let r = f(700_000 + seed * 31 + event_index as u64 * 17);
    let pk = sk * f(SCHNORR_G);
    let r_point = r * f(SCHNORR_G);
    let challenge = toy_challenge(r_point, pk, f(msg));
    let s = r + challenge * sk;
    ToySignature {
        msg,
        sk,
        pk,
        r_point,
        s,
    }
}
