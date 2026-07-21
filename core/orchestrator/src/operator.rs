//! The deterministic "Operator" policy: watches ride queue lengths across
//! recent snapshots and proposes a one-step price change when a
//! consistent pattern holds. No LLM, no learning -- a fixed rule read
//! against `governor::Constitution`'s thresholds. This module only
//! *proposes*; `core/governor` decides whether a proposal is authorized.

use std::collections::VecDeque;

use common::protocol::{ObservationSnapshot, Ride};
use governor::{Constitution, Proposal};
use serde_json::json;

/// For every ride in the latest snapshot, proposes a one-`price_step`
/// price increase if its `queue_length` has exceeded
/// `queue_length_high_threshold` for `consecutive_snapshots_required`
/// snapshots in a row (and the resulting price would stay within
/// `price_bounds`), or a symmetric decrease if it's been at or below
/// `queue_length_low_threshold` for that many snapshots. Pure: no I/O,
/// takes history by reference, returns proposals for the caller
/// (core/orchestrator's connection handler) to submit to the governor.
pub fn propose_price_changes(
    history: &VecDeque<ObservationSnapshot>,
    constitution: &Constitution,
    current_tick: u64,
) -> Vec<Proposal> {
    let Some(latest) = history.back() else {
        return Vec::new();
    };
    let n = constitution.consecutive_snapshots_required;
    if n == 0 || history.len() < n {
        return Vec::new();
    }

    let mut proposals = Vec::new();
    for ride in &latest.rides {
        let recent: Vec<&Ride> = history
            .iter()
            .rev()
            .take(n)
            .filter_map(|snapshot| snapshot.rides.iter().find(|r| r.id == ride.id))
            .collect();
        if recent.len() < n {
            // This ride hasn't existed for the full window (e.g. just
            // built) -- not enough history to judge a consistent pattern.
            continue;
        }

        let all_high = recent
            .iter()
            .all(|r| r.queue_length > constitution.queue_length_high_threshold);
        let all_low = recent
            .iter()
            .all(|r| r.queue_length <= constitution.queue_length_low_threshold);

        if all_high {
            let proposed_price = ride.price as i64 + constitution.price_step;
            if proposed_price <= constitution.price_bounds.max {
                proposals.push(price_increase_proposal(
                    ride,
                    proposed_price,
                    constitution,
                    current_tick,
                ));
            }
        } else if all_low {
            let proposed_price = ride.price as i64 - constitution.price_step;
            if proposed_price >= constitution.price_bounds.min {
                proposals.push(price_decrease_proposal(
                    ride,
                    proposed_price,
                    constitution,
                    current_tick,
                ));
            }
        }
    }
    proposals
}

const CONFIDENCE: f32 = 0.8;
const EXPIRY_WINDOW_TICKS: u64 = 2000;

fn price_increase_proposal(
    ride: &Ride,
    proposed_price: i64,
    constitution: &Constitution,
    current_tick: u64,
) -> Proposal {
    Proposal {
        agent: "The Operator".to_string(),
        assumptions: json!({
            "rule": "queue_too_long",
            "ride_id": ride.id,
            "ride_name": ride.name,
            "queue_length_high_threshold": constitution.queue_length_high_threshold,
            "consecutive_snapshots_required": constitution.consecutive_snapshots_required,
            "current_price": ride.price,
        }),
        predicted_effect: json!({ "queue_length": "expected to decrease as demand-priced guests self-select out" }),
        confidence: CONFIDENCE,
        cost_envelope: json!({ "max_price_delta": constitution.price_step }),
        expiry_tick: current_tick + EXPIRY_WINDOW_TICKS,
        ride_id: ride.id,
        proposed_price,
    }
}

fn price_decrease_proposal(
    ride: &Ride,
    proposed_price: i64,
    constitution: &Constitution,
    current_tick: u64,
) -> Proposal {
    Proposal {
        agent: "The Operator".to_string(),
        assumptions: json!({
            "rule": "queue_empty",
            "ride_id": ride.id,
            "ride_name": ride.name,
            "queue_length_low_threshold": constitution.queue_length_low_threshold,
            "consecutive_snapshots_required": constitution.consecutive_snapshots_required,
            "current_price": ride.price,
        }),
        predicted_effect: json!({ "guest_count": "expected to increase as a lower price attracts more riders" }),
        confidence: CONFIDENCE,
        cost_envelope: json!({ "max_price_delta": constitution.price_step }),
        expiry_tick: current_tick + EXPIRY_WINDOW_TICKS,
        ride_id: ride.id,
        proposed_price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::protocol::{ParkDate, RideStatus, StaffCounts, Weather};

    fn constitution() -> Constitution {
        Constitution {
            policy_version: "test".to_string(),
            daily_action_budget: 20,
            max_actions_per_hour: 10,
            min_confidence: 0.6,
            price_bounds: governor::PriceBounds { min: 0, max: 10 },
            per_ride_cooldown_ticks: 1000,
            queue_length_high_threshold: 5,
            queue_length_low_threshold: 0,
            consecutive_snapshots_required: 2,
            price_step: 1,
        }
    }

    fn snapshot_with_ride(ride_id: u32, queue_length: u32, price: u32) -> ObservationSnapshot {
        ObservationSnapshot {
            park_date: ParkDate {
                year: 1,
                month: 1,
                day: 1,
            },
            cash: 0,
            loan: 0,
            park_rating: 500,
            guest_count: 0,
            rides: vec![Ride {
                id: ride_id,
                name: "Test Ride".to_string(),
                kind: "test".to_string(),
                status: RideStatus::Open,
                price,
                queue_length,
                downtime: 0,
            }],
            staff_counts: StaffCounts {
                handyman: 0,
                mechanic: 0,
                security: 0,
                entertainer: 0,
            },
            weather: Weather::Sunny,
        }
    }

    #[test]
    fn no_proposal_without_enough_history() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 10, 5));
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert!(
            proposals.is_empty(),
            "1 snapshot is fewer than consecutive_snapshots_required=2"
        );
    }

    #[test]
    fn proposes_price_increase_when_queue_consistently_high() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 10, 5));
        history.push_back(snapshot_with_ride(0, 8, 5));
        let proposals = propose_price_changes(&history, &constitution(), 100);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].ride_id, 0);
        assert_eq!(proposals[0].proposed_price, 6);
        assert_eq!(proposals[0].expiry_tick, 100 + EXPIRY_WINDOW_TICKS);
    }

    #[test]
    fn proposes_price_decrease_when_queue_consistently_empty() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 0, 5));
        history.push_back(snapshot_with_ride(0, 0, 5));
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].proposed_price, 4);
    }

    #[test]
    fn no_proposal_when_queue_is_inconsistent() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 10, 5)); // high
        history.push_back(snapshot_with_ride(0, 0, 5)); // low -- breaks the streak either way
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert!(proposals.is_empty());
    }

    #[test]
    fn no_price_increase_proposal_past_the_max_bound() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 10, 10)); // already at price_bounds.max
        history.push_back(snapshot_with_ride(0, 10, 10));
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert!(
            proposals.is_empty(),
            "must not propose a price above price_bounds.max"
        );
    }

    #[test]
    fn no_price_decrease_proposal_past_the_min_bound() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(0, 0, 0)); // already at price_bounds.min
        history.push_back(snapshot_with_ride(0, 0, 0));
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert!(
            proposals.is_empty(),
            "must not propose a price below price_bounds.min"
        );
    }

    #[test]
    fn a_ride_missing_from_part_of_the_window_is_skipped() {
        let mut history = VecDeque::new();
        history.push_back(snapshot_with_ride(1, 10, 5)); // different ride_id -- ride 0 didn't exist yet
        history.push_back(snapshot_with_ride(0, 10, 5));
        let proposals = propose_price_changes(&history, &constitution(), 0);
        assert!(proposals.is_empty());
    }
}
