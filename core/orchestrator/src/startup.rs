//! Crash-recovery reconciliation, run once at orchestrator startup --
//! see docs/DECISIONS.md ADR-0006's crash-recovery section.

use governor::SafetyState;
use sqlx::PgPool;

use crate::db;
use crate::error::OrchestratorError;

/// Logs (but does not attempt to resume) any actions orphaned by a crash
/// between sending a `command.request` and receiving its result, then
/// unconditionally enters `Cautious` -- a fresh start is inherently
/// unconfirmed territory whether or not this is actually a post-crash
/// restart, so there is no "was this a clean shutdown" check to get
/// wrong.
pub async fn reconcile_on_startup(pool: &PgPool) -> Result<(), OrchestratorError> {
    let in_flight = db::find_in_flight_actions(pool).await?;
    if !in_flight.is_empty() {
        tracing::warn!(
            count = in_flight.len(),
            idempotency_keys = ?in_flight.iter().map(|a| &a.idempotency_key).collect::<Vec<_>>(),
            "found actions from a previous run with no recorded result; their outcome is unknown"
        );
    }

    let from_state = db::current_safety_state(pool, None, 0).await?;
    db::insert_state_transition(
        pool,
        None,
        from_state,
        SafetyState::Cautious,
        "orchestrator startup",
        "orchestrator",
        None,
    )
    .await?;
    tracing::info!(from = from_state.as_str(), "entering cautious on startup");

    Ok(())
}
