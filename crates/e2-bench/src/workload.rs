use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use rand_distr::{Distribution, Normal};

#[derive(Clone, Debug)]
pub struct EpcisEvent {
    pub event_id: usize,
    pub ingest_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Lot {
    pub lot_id: usize,
    pub events: Vec<EpcisEvent>,
    pub lot_ready_at_ms: u64,
}

/// Generates a Poisson event stream deterministically for a given seed.
/// `lambda_events_per_min`: average arrival rate (events per minute).
/// `duration_min`: total stream duration in minutes.
pub fn generate_poisson_event_stream(
    seed: u64,
    lambda_events_per_min: f64,
    duration_min: f64,
) -> Vec<EpcisEvent> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ 0x00E2_E2E2);
    let total_duration_ms = (duration_min * 60_000.0) as u64;
    let lambda_per_ms = lambda_events_per_min / 60_000.0;

    let mut events = Vec::new();
    let mut current_time_ms = 0u64;
    let mut event_id = 1usize;

    while current_time_ms < total_duration_ms {
        // Draw exponential inter-arrival time: -ln(1 - U) / lambda
        let u: f64 = rng.gen_range(0.0..1.0);
        // Avoid u = 1.0 (ln(0) = -inf) or u = 0.0 (ln(1) = 0, delta = 0)
        let clamped_u: f64 = u.clamp(1e-10, 1.0 - 1e-10);
        let delta_ms = (- (1.0f64 - clamped_u).ln() / lambda_per_ms).round() as u64;
        let delta_ms = delta_ms.max(1); // At least 1ms separation

        current_time_ms += delta_ms;
        if current_time_ms >= total_duration_ms {
            break;
        }

        events.push(EpcisEvent {
            event_id,
            ingest_at_ms: current_time_ms,
        });
        event_id += 1;
    }

    events
}

/// Accumulates events into lots according to truncated normal N(24, 8) in [8, 64].
pub fn accumulate_lots(events: &[EpcisEvent], seed: u64) -> Vec<Lot> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ 0x00E2_1007);
    let normal_dist = Normal::new(24.0, 8.0).expect("Valid normal dist parameters");

    let mut lots = Vec::new();
    let mut lot_id = 1usize;
    let mut event_idx = 0usize;

    while event_idx < events.len() {
        // Sample target lot size in [8, 64]
        let target_size = loop {
            let sample_val: f64 = normal_dist.sample(&mut rng);
            let val = sample_val.round() as i64;
            if (8..=64).contains(&val) {
                break val as usize;
            }
        };

        let end_idx = (event_idx + target_size).min(events.len());
        let lot_events = events[event_idx..end_idx].to_vec();

        // Lot ready time is the ingestion timestamp of the last event in this lot
        let lot_ready_at_ms = lot_events.last().map(|e| e.ingest_at_ms).unwrap_or(0);

        lots.push(Lot {
            lot_id,
            events: lot_events,
            lot_ready_at_ms,
        });

        lot_id += 1;
        event_idx = end_idx;
    }

    lots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_event_generation() {
        let stream1 = generate_poisson_event_stream(42, 480.0, 5.0);
        let stream2 = generate_poisson_event_stream(42, 480.0, 5.0);
        assert!(!stream1.is_empty());
        assert_eq!(stream1.len(), stream2.len());
        for (e1, e2) in stream1.iter().zip(stream2.iter()) {
            assert_eq!(e1.event_id, e2.event_id);
            assert_eq!(e1.ingest_at_ms, e2.ingest_at_ms);
        }
    }

    #[test]
    fn test_lot_size_bounds() {
        let stream = generate_poisson_event_stream(123, 480.0, 60.0);
        let lots = accumulate_lots(&stream, 123);
        assert!(!lots.is_empty());

        for (i, lot) in lots.iter().enumerate() {
            let size = lot.events.len();
            if i == lots.len() - 1 {
                // Last lot can be smaller if stream ends
                assert!(size >= 1 && size <= 64);
            } else {
                assert!(size >= 8 && size <= 64, "Lot size {} out of bounds [8, 64]", size);
            }
        }
    }
}
