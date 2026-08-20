use crate::epcis::{EpcisEventV1, PointE6, SignedEpcisEventV1, SimplePolygon};
use crate::types::{f, Profile, F, POSEIDON_TAG_ACTOR, THRESHOLD};
use ed25519_dalek::SigningKey;
use p3_field::PrimeCharacteristicRing;
use p3_symmetric::Permutation;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticLot {
    pub readings: Vec<u64>,
    pub timestamps: Vec<u64>,
    pub coords: Vec<PointE6>,
    pub events: Vec<EpcisEventV1>,
    pub signatures: Vec<[u8; 64]>,
    pub cert_ids: Vec<u64>,
    pub lot_id: u64,
    pub secret: u64,
    pub actor_id: u64,
    pub actor_secret: u64,
    pub role_tag: u64,
    pub epoch: u64,
    pub polygon: SimplePolygon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HalfPlane {
    pub a: i64,
    pub b: i64,
    pub c: i64,
}

pub fn synthetic_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ ((events as u64) << 32) ^ profile_tag(profile));
    let readings: Vec<u64> = (0..events)
        .map(|_| rng.gen_range(100..=THRESHOLD))
        .collect();
    let timestamps: Vec<u64> = (0..events)
        .map(|i| 1_700_000_000 + seed * 1000 + i as u64 * 15)
        .collect();
    let cert_ids: Vec<u64> = (0..events)
        .map(|_| rng.gen_range(10_000..=999_999))
        .collect();
    let lot_id = rng.gen_range(1_000_000..=9_999_999);
    let secret = rng.gen_range(100_000_000..=999_999_999);
    let epoch = 1_800_000_000 + seed;
    let actor_id = rng.gen_range(10_000..=99_999);
    let actor_secret = rng.gen_range(100_000_000..=999_999_999);
    let role_tag = POSEIDON_TAG_ACTOR;
    let mut actor_secret_key = [0u8; 32];
    rng.fill(&mut actor_secret_key);
    let actor_signing_key = SigningKey::from_bytes(&actor_secret_key);
    let actor_public_key = actor_signing_key.verifying_key().to_bytes();
    let polygon =
        SimplePolygon::new(profile.polygon()).expect("built-in benchmark polygon is valid");
    let coords = compliant_points_outside_forbidden_polygon(&mut rng, events, profile);
    let epcis_events = (0..events)
        .map(|index| EpcisEventV1 {
            event_id: (seed << 16) | index as u64,
            lot_id,
            epoch_id: epoch,
            timestamp_ms: timestamps[index] * 1_000,
            readings: readings[index] as u32,
            latitude_e6: coords[index].latitude_e6,
            longitude_e6: coords[index].longitude_e6,
            certificate_id: cert_ids[index],
            role: role_tag as u8,
            actor_public_key,
        })
        .collect::<Vec<_>>();
    let signatures = epcis_events
        .iter()
        .cloned()
        .map(|event| SignedEpcisEventV1::sign(event, &actor_signing_key).signature)
        .collect();

    SyntheticLot {
        readings,
        timestamps,
        coords,
        events: epcis_events,
        signatures,
        cert_ids,
        lot_id,
        secret,
        actor_id,
        actor_secret,
        role_tag,
        epoch,
        polygon,
    }
}

fn profile_tag(profile: Profile) -> u64 {
    match profile {
        Profile::CoffeeSmall => 0xC0FFEE01,
        Profile::CoffeeDefault => 0xC0FFEE02,
        Profile::Stress => 0xC0FFEE03,
    }
}

fn compliant_points_outside_forbidden_polygon(
    rng: &mut ChaCha20Rng,
    events: usize,
    profile: Profile,
) -> Vec<PointE6> {
    let (lat0, lat1, lon0, lon1) = match profile {
        Profile::CoffeeSmall => (10_820_000, 10_840_000, 106_750_000, 106_770_000),
        Profile::CoffeeDefault => (10_825_000, 10_845_000, 106_755_000, 106_775_000),
        Profile::Stress => (10_830_000, 10_850_000, 106_760_000, 106_780_000),
    };
    (0..events)
        .map(|_| PointE6::new(rng.gen_range(lat0..=lat1), rng.gen_range(lon0..=lon1)))
        .collect()
}

pub fn inside_forbidden_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if let Some(first) = lot.coords.first_mut() {
        *first = match profile {
            Profile::CoffeeSmall => PointE6::new(10_775_000, 106_680_000),
            Profile::CoffeeDefault => PointE6::new(10_780_000, 106_680_000),
            Profile::Stress => PointE6::new(10_780_000, 106_680_000),
        };
        lot.events[0].latitude_e6 = first.latitude_e6;
        lot.events[0].longitude_e6 = first.longitude_e6;
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

pub fn non_monotonic_timestamp_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if lot.timestamps.len() > 1 {
        lot.timestamps[1] = lot.timestamps[0].saturating_sub(1);
    }
    lot
}

pub fn equal_timestamp_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    if lot.timestamps.len() > 1 {
        lot.timestamps[1] = lot.timestamps[0];
    }
    lot
}

pub fn tampered_actor_lot(seed: u64, events: usize, profile: Profile) -> SyntheticLot {
    let mut lot = synthetic_lot(seed, events, profile);
    lot.actor_secret += 1;
    lot
}

pub fn halfplanes_for_polygon(poly: &[PointE6]) -> Vec<HalfPlane> {
    let mut planes = Vec::with_capacity(poly.len());
    for i in 0..poly.len() {
        let start = poly[i];
        let end = poly[(i + 1) % poly.len()];
        let x1 = start.longitude_e6;
        let y1 = start.latitude_e6;
        let x2 = end.longitude_e6;
        let y2 = end.latitude_e6;
        let dx = x2 as i64 - x1 as i64;
        let dy = y2 as i64 - y1 as i64;
        let a = -(dy);
        let b = dx;
        let c = dy * x1 as i64 - dx * y1 as i64;
        planes.push(HalfPlane { a, b, c });
    }
    planes
}

pub fn poseidon2_permute(input: [F; 8]) -> [F; 8] {
    let external = p3_poseidon2::ExternalLayerConstants::new(
        p3_goldilocks::GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_INITIAL.to_vec(),
        p3_goldilocks::GOLDILOCKS_POSEIDON2_RC_8_EXTERNAL_FINAL.to_vec(),
    );
    let perm = p3_goldilocks::Poseidon2Goldilocks::<8>::new(
        &external,
        &p3_goldilocks::GOLDILOCKS_POSEIDON2_RC_8_INTERNAL,
    );
    perm.permute(input)
}

pub fn poseidon2_hash(inputs: &[F]) -> [F; 4] {
    let mut state = [F::ZERO; 8];
    for (i, value) in inputs.iter().take(7).enumerate() {
        state[i] = *value;
    }
    state[7] = f(inputs.len() as u64);
    let out = poseidon2_permute(state);
    [out[0], out[1], out[2], out[3]]
}
