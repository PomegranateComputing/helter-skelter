use std::path::Path;

use serde::Deserialize;

use crate::error::GovernorError;

#[derive(Debug, Clone, Deserialize)]
pub struct PriceBounds {
    pub min: i64,
    pub max: i64,
}

/// Governor policy for milestone 0.1, loaded from
/// config/constitution-0.1.yaml. See that file for the meaning of each
/// field and docs/DECISIONS.md ADR-0004 for why these particular
/// constraints and not others.
#[derive(Debug, Clone, Deserialize)]
pub struct Constitution {
    pub policy_version: String,
    pub daily_action_budget: u32,
    pub max_actions_per_hour: u32,
    pub min_confidence: f32,
    pub price_bounds: PriceBounds,
    pub per_ride_cooldown_ticks: u64,
    pub queue_length_high_threshold: u32,
    pub queue_length_low_threshold: u32,
    pub consecutive_snapshots_required: usize,
    pub price_step: i64,
    pub snapshot_max_age_ticks: u64,
    pub max_unexpected_cash_drop: i64,
    pub conservation_ticks: u64,
    pub cautious_recovery_heartbeats: u32,
    pub oscillation_window_ticks: u64,
    pub oscillation_max_reversals: u32,
    pub db_unreachable_stopped_after_secs: u64,
    pub watchdog_poll_interval_secs: u64,
    pub action_rate_stopped_threshold_per_minute: u32,
}

impl Constitution {
    pub fn load(path: &Path) -> Result<Self, GovernorError> {
        let text =
            std::fs::read_to_string(path).map_err(|source| GovernorError::ReadConstitution {
                path: path.display().to_string(),
                source,
            })?;
        serde_yaml::from_str(&text).map_err(|source| GovernorError::ParseConstitution {
            path: path.display().to_string(),
            source,
        })
    }
}
