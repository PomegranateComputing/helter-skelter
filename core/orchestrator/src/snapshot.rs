//! Snapshot policy: "ensure a snapshot no older than N ticks exists
//! before authorizing an action." Delegates the actual file production to
//! `scripts/dev/snapshot.sh`, a host-side script -- neither the plugin nor
//! the OpenRCT2 CLI can trigger a save on demand in v0.5.3 (see
//! docs/OPENRCT2_INTEGRATION.md's save-triggering GAP and
//! docs/DECISIONS.md ADR-0005). The script copies whichever autosave the
//! engine has most recently written; this module just decides *when* a
//! fresh one is needed and records where it ended up.

use std::path::Path;

use sqlx::PgPool;
use uuid::Uuid;

use crate::db;
use crate::error::OrchestratorError;

/// The park `scripts/dev/run-stack.sh` prefers over the static dev park
/// if present -- restoring a rollback here is what makes the *next*
/// `openrct2-cli` start pick it up (see this module's `restore_snapshot`
/// doc comment and docs/DECISIONS.md ADR-0005).
pub const CURRENT_PARK_PATH: &str = "runtime/current-park.park";

/// Where to look for a fresh-enough snapshot before authorizing an
/// action, and how to produce one if none exists.
pub struct SnapshotConfig {
    pub script_path: std::path::PathBuf,
    pub checkpoint_root: std::path::PathBuf,
}

/// Returns the id of a snapshot no older than `max_age_ticks` for
/// `simulation_id`, taking a fresh one via `scripts/dev/snapshot.sh` if
/// the latest recorded one (if any) is older than that, or none exists
/// yet.
pub async fn ensure_recent_snapshot(
    pool: &PgPool,
    config: &SnapshotConfig,
    simulation_id: Uuid,
    current_tick: u64,
    max_age_ticks: u64,
) -> Result<Uuid, OrchestratorError> {
    if let Some((id, tick)) = db::latest_snapshot(pool, simulation_id).await? {
        if current_tick.saturating_sub(tick as u64) <= max_age_ticks {
            return Ok(id);
        }
    }
    take_snapshot(pool, config, simulation_id, current_tick).await
}

async fn take_snapshot(
    pool: &PgPool,
    config: &SnapshotConfig,
    simulation_id: Uuid,
    current_tick: u64,
) -> Result<Uuid, OrchestratorError> {
    let dest_dir = config.checkpoint_root.join(simulation_id.to_string());
    tokio::fs::create_dir_all(&dest_dir).await?;

    let output = tokio::process::Command::new(&config.script_path)
        .arg(&dest_dir)
        .arg(current_tick.to_string())
        .output()
        .await?;

    if !output.status.success() {
        return Err(OrchestratorError::SnapshotScriptFailed {
            script: config.script_path.display().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let storage_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    db::insert_snapshot(pool, simulation_id, "autosave", &storage_path, current_tick)
        .await
        .map_err(OrchestratorError::Db)
}

/// Restores `snapshot_id`'s file to `dest` (the park the next
/// `openrct2-cli` invocation loads -- see `scripts/dev/run-stack.sh`'s
/// `runtime/current-park.park` convention) and records the rollback.
/// Used by both the manual `orchestrator rollback --to` CLI subcommand
/// and the automatic trigger in tcp_server.rs. Either way this only ever
/// affects which park the *next* `openrct2-cli` start loads -- there is
/// no way to hot-swap a running instance's world state from a plugin (the
/// same GAP that blocks an on-demand save), so the live process the
/// rollback was triggered against keeps running on its current state
/// until it's actually restarted. See docs/DECISIONS.md ADR-0005.
pub async fn restore_snapshot(
    pool: &PgPool,
    snapshot_id: Uuid,
    dest: &Path,
    reason: &str,
    triggered_by: &str,
) -> Result<Uuid, OrchestratorError> {
    let snapshot = db::find_snapshot(pool, snapshot_id)
        .await?
        .ok_or(OrchestratorError::SnapshotNotFound(snapshot_id))?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(&snapshot.storage_path, dest).await?;

    db::insert_rollback(
        pool,
        snapshot.simulation_id,
        snapshot_id,
        reason,
        triggered_by,
    )
    .await
    .map_err(OrchestratorError::Db)
}
