use std::path::Path;

use serde::Deserialize;

use crate::error::OrchestratorError;

/// Only the fields the orchestrator needs from config/bridge.json --
/// unknown fields (heartbeatIntervalTicks, etc., which are bridge-only
/// settings) are ignored rather than rejected, so both sides can read the
/// same file as their single source of truth for the port they must agree
/// on without either owning fields it doesn't use.
#[derive(Debug, Deserialize)]
pub struct BridgeConfig {
    pub host: String,
    pub port: u16,
}

pub fn load(path: &Path) -> Result<BridgeConfig, OrchestratorError> {
    let text = std::fs::read_to_string(path).map_err(|source| OrchestratorError::ReadConfig {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| OrchestratorError::ParseConfig {
        path: path.display().to_string(),
        source,
    })
}
