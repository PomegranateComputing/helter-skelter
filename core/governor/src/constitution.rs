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
