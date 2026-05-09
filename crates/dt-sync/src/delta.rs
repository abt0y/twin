//! Delta sync: compute and apply event deltas between peers.
//!
//! The sync protocol is: each peer sends its vector clock;
//! the other peer replies with all events that dominate the
//! requesting peer's clock (i.e., events the requester has not seen).

use crate::vector_clock::VectorClock;

/// A delta bundle of events to be sent to a peer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeltaBundle {
    pub from_node: String,
    pub to_node: String,
    pub since_clock: VectorClock,
    pub events: Vec<serde_json::Value>,
    pub bundle_hash: String,
}

/// Compute a delta: given a target vector clock, return all events
/// that are not causally dominated by that clock.
pub fn compute_delta<E, F>(
    all_events: &[E],
    peer_clock: &VectorClock,
    event_clock_extractor: F,
) -> Vec<E>
where
    E: Clone,
    F: Fn(&E) -> VectorClock,
{
    all_events
        .iter()
        .filter(|e| {
            let ev_clock = event_clock_extractor(e);
            // Include if event is not dominated by peer_clock
            match peer_clock.compare(&ev_clock) {
                Some(std::cmp::Ordering::Less) => true,
                Some(std::cmp::Ordering::Equal) => false,
                Some(std::cmp::Ordering::Greater) => false,
                None => true, // concurrent: must send
            }
        })
        .cloned()
        .collect()
}

/// Apply a delta: merge vector clocks, append events to local log.
pub fn apply_delta(
    local_clock: &mut VectorClock,
    remote_clock: &VectorClock,
) {
    local_clock.merge(remote_clock);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_delta() {
        let mut peer_clock = VectorClock::new("node-a".into());
        peer_clock.increment();

        let mut ev_clock = VectorClock::new("node-a".into());
        ev_clock.increment();
        ev_clock.increment();

        let events = vec![("ev1", peer_clock.clone()), ("ev2", ev_clock)];
        let delta = compute_delta(&events, &peer_clock, |e| e.1.clone());
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].0, "ev2");
    }
}
