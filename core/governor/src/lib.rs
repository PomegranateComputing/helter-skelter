//! Policy engine: answers "is this proposal authorized?" against
//! config/constitution-0.1.yaml, and nothing else -- it doesn't propose
//! anything itself (that's core/orchestrator's operator.rs) and doesn't
//! execute anything (that's the bridge). Every call to [`Governor::authorize`]
//! returns a fully-reasoned [`Authorization`], including when the answer is
//! "no action justified".

mod authorization;
mod constitution;
mod error;
mod proposal;
mod safety_state;

pub use authorization::{Authorization, Decision};
pub use constitution::{Constitution, PriceBounds};
pub use error::GovernorError;
pub use proposal::Proposal;
pub use safety_state::SafetyState;

use std::collections::HashMap;
use std::time::{Duration, Instant};

const ONE_HOUR: Duration = Duration::from_secs(3600);
const ONE_DAY: Duration = Duration::from_secs(24 * 3600);

/// Tracks enough state to enforce rate limits and cooldowns across calls.
/// This state is in-memory only and does not survive an orchestrator
/// restart -- a known 0.1 simplification, see docs/DECISIONS.md ADR-0004.
/// The system-wide [`SafetyState`] gate (Normal/Cautious/Conservation/
/// Quarantine/Rollback/Stopped) is deliberately *not* tracked here --
/// see docs/DECISIONS.md ADR-0006 for why that moved to being backed by
/// Postgres (core/orchestrator/src/db.rs's `latest_safety_state`) and
/// checked by the caller before `authorize` is ever invoked, rather than
/// being in-memory governor state like phase 7's `Mode` was.
pub struct Governor {
    constitution: Constitution,
    authorized_at: Vec<Instant>,
    per_ride_last_authorized_tick: HashMap<u32, u64>,
}

impl Governor {
    pub fn new(constitution: Constitution) -> Self {
        Self {
            constitution,
            authorized_at: Vec::new(),
            per_ride_last_authorized_tick: HashMap::new(),
        }
    }

    pub fn constitution(&self) -> &Constitution {
        &self.constitution
    }

    pub fn policy_version(&self) -> &str {
        &self.constitution.policy_version
    }

    /// Checks `proposal` against every constraint in the constitution, in
    /// order, returning the first failing reason -- or an authorization if
    /// all pass. `current_tick` is the simulation tick the proposal was
    /// made at (used for the per-ride cooldown, which is a simulation-time
    /// concept; the hourly/daily budgets are wall-clock, since "per hour"
    /// doesn't make sense at variable game speed). Callers must check the
    /// current `SafetyState` themselves first -- see this struct's doc
    /// comment.
    pub fn authorize(&mut self, proposal: &Proposal, current_tick: u64) -> Authorization {
        let policy_version = self.constitution.policy_version.clone();

        if proposal.confidence < self.constitution.min_confidence {
            return self.reject(
                policy_version,
                format!(
                    "confidence {:.2} is below min_confidence {:.2}",
                    proposal.confidence, self.constitution.min_confidence
                ),
            );
        }

        let bounds = &self.constitution.price_bounds;
        if proposal.proposed_price < bounds.min || proposal.proposed_price > bounds.max {
            return self.reject(
                policy_version,
                format!(
                    "proposed price {} is outside price_bounds [{}, {}]",
                    proposal.proposed_price, bounds.min, bounds.max
                ),
            );
        }

        if let Some(&last_tick) = self.per_ride_last_authorized_tick.get(&proposal.ride_id) {
            let elapsed = current_tick.saturating_sub(last_tick);
            if elapsed < self.constitution.per_ride_cooldown_ticks {
                return self.reject(
                    policy_version,
                    format!(
                        "ride {} is on cooldown: {} ticks elapsed, {} required",
                        proposal.ride_id, elapsed, self.constitution.per_ride_cooldown_ticks
                    ),
                );
            }
        }

        let now = Instant::now();
        self.authorized_at
            .retain(|&t| now.duration_since(t) < ONE_DAY);

        let last_hour_count = self
            .authorized_at
            .iter()
            .filter(|&&t| now.duration_since(t) < ONE_HOUR)
            .count();
        if last_hour_count >= self.constitution.max_actions_per_hour as usize {
            return self.reject(
                policy_version,
                format!(
                    "max_actions_per_hour ({}) already reached",
                    self.constitution.max_actions_per_hour
                ),
            );
        }

        if self.authorized_at.len() >= self.constitution.daily_action_budget as usize {
            return self.reject(
                policy_version,
                format!(
                    "daily_action_budget ({}) already reached",
                    self.constitution.daily_action_budget
                ),
            );
        }

        self.authorized_at.push(now);
        self.per_ride_last_authorized_tick
            .insert(proposal.ride_id, current_tick);

        Authorization {
            decision: Decision::Authorized,
            reason: "within budget, confidence, price bounds, and cooldown".to_string(),
            policy_version,
        }
    }

    fn reject(&self, policy_version: String, reason: String) -> Authorization {
        Authorization {
            decision: Decision::Rejected,
            reason,
            policy_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn constitution() -> Constitution {
        Constitution {
            policy_version: "test-0.1.0".to_string(),
            daily_action_budget: 3,
            max_actions_per_hour: 2,
            min_confidence: 0.6,
            price_bounds: PriceBounds { min: 0, max: 10 },
            per_ride_cooldown_ticks: 1000,
            queue_length_high_threshold: 5,
            queue_length_low_threshold: 0,
            consecutive_snapshots_required: 2,
            price_step: 1,
            snapshot_max_age_ticks: 2000,
            max_unexpected_cash_drop: 1000,
            conservation_ticks: 4000,
            cautious_recovery_heartbeats: 2,
            oscillation_window_ticks: 5000,
            oscillation_max_reversals: 3,
            db_unreachable_stopped_after_secs: 60,
            watchdog_poll_interval_secs: 5,
            action_rate_stopped_threshold_per_minute: 20,
        }
    }

    fn proposal(ride_id: u32, proposed_price: i64, confidence: f32) -> Proposal {
        Proposal {
            agent: "The Operator".to_string(),
            assumptions: json!({}),
            predicted_effect: json!({}),
            confidence,
            cost_envelope: json!({}),
            expiry_tick: 10_000,
            ride_id,
            proposed_price,
        }
    }

    #[test]
    fn authorizes_a_reasonable_proposal() {
        let mut gov = Governor::new(constitution());
        let auth = gov.authorize(&proposal(0, 5, 0.8), 0);
        assert_eq!(auth.decision, Decision::Authorized);
    }

    #[test]
    fn rejects_low_confidence() {
        let mut gov = Governor::new(constitution());
        let auth = gov.authorize(&proposal(0, 5, 0.5), 0);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(auth.reason.contains("confidence"));
    }

    #[test]
    fn rejects_price_outside_bounds() {
        let mut gov = Governor::new(constitution());
        let auth = gov.authorize(&proposal(0, 11, 0.8), 0);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(auth.reason.contains("price_bounds"));

        let auth = gov.authorize(&proposal(0, -1, 0.8), 0);
        assert_eq!(auth.decision, Decision::Rejected);
    }

    #[test]
    fn rejects_ride_on_cooldown() {
        let mut gov = Governor::new(constitution());
        assert_eq!(
            gov.authorize(&proposal(0, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        // Same ride, too soon (cooldown is 1000 ticks).
        let auth = gov.authorize(&proposal(0, 6, 0.8), 500);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(auth.reason.contains("cooldown"));
        // A different ride is unaffected by ride 0's cooldown.
        assert_eq!(
            gov.authorize(&proposal(1, 5, 0.8), 500).decision,
            Decision::Authorized
        );
    }

    #[test]
    fn cooldown_expires() {
        let mut gov = Governor::new(constitution());
        assert_eq!(
            gov.authorize(&proposal(0, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        let auth = gov.authorize(&proposal(0, 6, 0.8), 1000);
        assert_eq!(auth.decision, Decision::Authorized);
    }

    #[test]
    fn rejects_past_hourly_budget() {
        let mut gov = Governor::new(constitution());
        // max_actions_per_hour = 2; use different rides to avoid the
        // per-ride cooldown masking the budget check.
        assert_eq!(
            gov.authorize(&proposal(0, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        assert_eq!(
            gov.authorize(&proposal(1, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        let auth = gov.authorize(&proposal(2, 5, 0.8), 0);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(auth.reason.contains("max_actions_per_hour"));
    }

    #[test]
    fn rejects_past_daily_budget() {
        // daily_action_budget = 3, but max_actions_per_hour = 2 would trip
        // first -- construct a constitution with a high hourly limit to
        // isolate the daily check.
        let mut c = constitution();
        c.max_actions_per_hour = 100;
        c.daily_action_budget = 2;
        let mut gov = Governor::new(c);
        assert_eq!(
            gov.authorize(&proposal(0, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        assert_eq!(
            gov.authorize(&proposal(1, 5, 0.8), 0).decision,
            Decision::Authorized
        );
        let auth = gov.authorize(&proposal(2, 5, 0.8), 0);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(auth.reason.contains("daily_action_budget"));
    }

    #[test]
    fn can_answer_no_action_justified_with_a_clear_reason() {
        let mut gov = Governor::new(constitution());
        let auth = gov.authorize(&proposal(0, 5, 0.1), 0);
        assert_eq!(auth.decision, Decision::Rejected);
        assert!(!auth.reason.is_empty());
    }
}
