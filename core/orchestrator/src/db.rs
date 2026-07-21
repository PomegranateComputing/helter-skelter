//! Durable persistence of observations, simulation lifecycle events, and
//! the decision pipeline (proposal/authorization/action/action_result).
//!
//! Observations go through [`Persistence`], a single background worker fed
//! by a bounded channel: if the database is unreachable, jobs queue up
//! (bounded, so a sustained outage can't grow memory unboundedly) and
//! retry with backoff, and connection handling continues uninterrupted --
//! a DB outage degrades the reported `db_state` to CAUTIOUS, it never
//! crashes the orchestrator or blocks the bridge connection.
//!
//! Simulation-start and the decision pipeline are synchronous instead
//! (see the second half of this file) -- see docs/DECISIONS.md ADR-0004
//! for why: simulation-start because everything else foreign-keys to it
//! (a buffered, possibly-delayed insert here would make a proposal's
//! insert fail with a FK violation depending on how the two async tasks
//! happen to interleave -- a real bug this design hit and fixed, not a
//! hypothetical), and the decision pipeline because "we authorized this
//! but can't prove it" is worse than not authorizing it.

use std::time::Duration;

use chrono::{DateTime, Utc};
use governor::{Authorization, Decision, Proposal, SafetyState};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::OrchestratorError;
use crate::state::{DbState, Shared};

/// Bounded so a sustained DB outage can't grow memory without limit --
/// past this many pending jobs, new submissions are dropped (logged),
/// which is the "CAUTIOUS" degradation the health endpoint reports.
const BUFFER_CAPACITY: usize = 500;

const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// A short `acquire_timeout` (sqlx's default is 30s) matters here more
/// than it would in a typical web service: a real DB outage chaos test
/// caught every heartbeat-triggered query blocking the *entire* bridge
/// connection's message loop for 30s each while Postgres was down --
/// nothing else on that connection (including the next
/// observation.snapshot) could be processed until each one timed out,
/// which is exactly the "DB outage degrades, never blocks" guarantee
/// ADR-0003 established for the buffered Persistence worker, quietly
/// broken for every *synchronous* query added since.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn connect(database_url: &str) -> Result<PgPool, OrchestratorError> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect(database_url)
        .await
        .map_err(OrchestratorError::Db)
}

#[derive(Debug)]
pub enum PersistJob {
    Observation {
        simulation_id: Uuid,
        message_id: Uuid,
        recorded_at: DateTime<Utc>,
        payload: Value,
        cash: i64,
        guest_count: i32,
        park_rating: i32,
    },
}

#[derive(Clone)]
pub struct Persistence {
    tx: mpsc::Sender<PersistJob>,
}

impl Persistence {
    /// Spawns the background worker and returns a handle to submit jobs
    /// to it. `shared`'s `db_state` is updated as the worker succeeds or
    /// fails to reach the database.
    pub fn spawn(pool: PgPool, shared: Shared) -> Self {
        let (tx, rx) = mpsc::channel(BUFFER_CAPACITY);
        tokio::spawn(run_worker(pool, rx, shared));
        Self { tx }
    }

    /// Never blocks the caller (the TCP connection handler): if the
    /// buffer is full, the job is dropped and logged rather than
    /// stalling message processing.
    pub fn submit(&self, job: PersistJob) {
        if self.tx.try_send(job).is_err() {
            tracing::warn!("persistence buffer full, dropping job");
        }
    }
}

async fn run_worker(pool: PgPool, mut rx: mpsc::Receiver<PersistJob>, shared: Shared) {
    while let Some(job) = rx.recv().await {
        let mut delay = INITIAL_RETRY_DELAY;
        loop {
            match apply_job(&pool, &job).await {
                Ok(()) => {
                    shared.write().await.db_state = DbState::Connected;
                    break;
                }
                Err(err) => {
                    shared.write().await.db_state = DbState::Cautious;
                    tracing::warn!(error = %err, "db write failed, retrying");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }
}

async fn apply_job(pool: &PgPool, job: &PersistJob) -> Result<(), sqlx::Error> {
    match job {
        PersistJob::Observation {
            simulation_id,
            message_id,
            recorded_at,
            payload,
            cash,
            guest_count,
            park_rating,
        } => {
            sqlx::query!(
                r#"
                INSERT INTO observations (simulation_id, message_id, recorded_at, payload, cash, guest_count, park_rating)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (message_id) DO NOTHING
                "#,
                simulation_id,
                message_id,
                recorded_at,
                payload,
                cash,
                guest_count,
                park_rating,
            )
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// --- Decision pipeline: synchronous, not buffered ------------------------
//
// Unlike observations/lifecycle events above, these writes are on the
// authorization critical path: if the governor can't durably record *why*
// it decided what it decided, the safe thing is to not act on that
// decision at all, not to buffer-and-retry silently while pretending the
// action already happened. Callers (tcp_server.rs) treat a failure here as
// "do not proceed" rather than routing it through Persistence.

/// Synchronous, not routed through [`Persistence`]: everything else in
/// this file's decision pipeline foreign-keys to `simulations`, so this
/// row must be committed before any of those inserts can succeed -- a
/// buffered insert here raced against the very next observation.snapshot's
/// proposal insert and failed with a foreign-key violation before this
/// was made synchronous.
pub async fn insert_simulation_start(
    pool: &PgPool,
    simulation_id: Uuid,
    bridge_version: &str,
    openrct2_version: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO simulations (id, bridge_version, openrct2_version)
        VALUES ($1, $2, $3)
        ON CONFLICT (id) DO NOTHING
        "#,
        simulation_id,
        bridge_version,
        openrct2_version,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_proposal(
    pool: &PgPool,
    simulation_id: Uuid,
    proposal: &Proposal,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO proposals (simulation_id, agent, assumptions, predicted_effect, confidence, cost_envelope, expiry_tick)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
        simulation_id,
        proposal.agent,
        proposal.assumptions,
        proposal.predicted_effect,
        proposal.confidence,
        proposal.cost_envelope,
        proposal.expiry_tick as i64,
    )
    .fetch_one(pool)
    .await
}

pub async fn insert_authorization(
    pool: &PgPool,
    proposal_id: Uuid,
    authorization: &Authorization,
) -> Result<Uuid, sqlx::Error> {
    let decision = match authorization.decision {
        Decision::Authorized => "authorized",
        Decision::Rejected => "rejected",
    };
    sqlx::query_scalar!(
        r#"
        INSERT INTO authorizations (proposal_id, decision, reason, policy_version)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        proposal_id,
        decision,
        authorization.reason,
        authorization.policy_version,
    )
    .fetch_one(pool)
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_action(
    pool: &PgPool,
    authorization_id: Uuid,
    command: &Value,
    idempotency_key: &str,
    expiry_tick: u64,
    tick: u64,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO actions (authorization_id, command, idempotency_key, expiry_tick, tick)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
        authorization_id,
        command,
        idempotency_key,
        expiry_tick as i64,
        tick as i64,
    )
    .fetch_one(pool)
    .await
}

pub struct InFlightAction {
    pub action_id: Uuid,
    pub idempotency_key: String,
}

/// Actions with no recorded `action_result` -- either still genuinely in
/// flight (rare, since results usually arrive within milliseconds) or,
/// far more likely on a restart, orphaned by a crash between sending the
/// `command.request` and receiving its `command.result`. Their outcome is
/// unknown; `idempotency_key` prevents ever double-executing them if a
/// future proposal happens to regenerate the same key, but there is no
/// mechanism to resume or query their result after the fact -- see
/// docs/DECISIONS.md ADR-0006's crash-recovery section for why entering
/// `Cautious` is the response, not an attempt to reconcile these
/// individually.
pub async fn find_in_flight_actions(pool: &PgPool) -> Result<Vec<InFlightAction>, sqlx::Error> {
    sqlx::query_as!(
        InFlightAction,
        r#"
        SELECT ac.id AS "action_id!", ac.idempotency_key AS "idempotency_key!"
        FROM actions ac
        LEFT JOIN action_results ar ON ar.action_id = ac.id
        WHERE ar.id IS NULL
        "#
    )
    .fetch_all(pool)
    .await
}

pub async fn insert_action_result(
    pool: &PgPool,
    action_id: Uuid,
    engine_cost: Option<i64>,
    engine_error: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO action_results (action_id, engine_cost, engine_error) VALUES ($1, $2, $3)",
        action_id,
        engine_cost,
        engine_error,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The most recent snapshot recorded for `simulation_id`, if any -- used
/// by `crate::snapshot::ensure_recent_snapshot` to decide whether a fresh
/// one is needed before authorizing an action.
pub async fn latest_snapshot(
    pool: &PgPool,
    simulation_id: Uuid,
) -> Result<Option<(Uuid, i64)>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT id, tick FROM snapshots WHERE simulation_id = $1 ORDER BY created_at DESC LIMIT 1",
        simulation_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.id, r.tick)))
}

pub async fn insert_snapshot(
    pool: &PgPool,
    simulation_id: Uuid,
    kind: &str,
    storage_path: &str,
    tick: u64,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO snapshots (simulation_id, kind, storage_path, tick) VALUES ($1, $2, $3, $4) RETURNING id",
        simulation_id,
        kind,
        storage_path,
        tick as i64,
    )
    .fetch_one(pool)
    .await
}

pub struct SnapshotRow {
    pub simulation_id: Uuid,
    pub storage_path: String,
    pub tick: i64,
}

/// Looks up a snapshot by id -- used by the `orchestrator rollback --to`
/// CLI subcommand to resolve which file to restore.
pub async fn find_snapshot(
    pool: &PgPool,
    snapshot_id: Uuid,
) -> Result<Option<SnapshotRow>, sqlx::Error> {
    let row = sqlx::query_as!(
        SnapshotRow,
        "SELECT simulation_id, storage_path, tick FROM snapshots WHERE id = $1",
        snapshot_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert_rollback(
    pool: &PgPool,
    simulation_id: Uuid,
    snapshot_id: Uuid,
    reason: &str,
    triggered_by: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO rollbacks (simulation_id, snapshot_id, reason, triggered_by) VALUES ($1, $2, $3, $4) RETURNING id",
        simulation_id,
        snapshot_id,
        reason,
        triggered_by,
    )
    .fetch_one(pool)
    .await
}

// --- Phase 8: the AFK safety-state machine -------------------------------
//
// The current state is derived by reading the latest row in
// `state_transitions`, not held as separate mutable state -- see
// docs/DECISIONS.md ADR-0006. `current_safety_state` is the one place
// that reads it, and it's self-healing: an expired Conservation window is
// resolved back to Normal (with its own logged transition) the moment
// anyone asks, rather than needing a background sweep.

#[allow(clippy::too_many_arguments)]
pub async fn insert_state_transition(
    pool: &PgPool,
    simulation_id: Option<Uuid>,
    from_state: SafetyState,
    to_state: SafetyState,
    reason: &str,
    triggered_by: &str,
    expires_at_tick: Option<u64>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO state_transitions (simulation_id, from_state, to_state, reason, triggered_by, expires_at_tick)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
        simulation_id,
        from_state.as_str(),
        to_state.as_str(),
        reason,
        triggered_by,
        expires_at_tick.map(|t| t as i64),
    )
    .fetch_one(pool)
    .await
}

/// The safety state right now, evaluated against `current_tick` and
/// scoped to `simulation_id`: the latest row that's either for this
/// simulation specifically or has no simulation_id at all (a transition
/// recorded before any bridge connected -- crash-recovery's startup
/// `Cautious`, or a watchdog check run with no simulation currently
/// live). This scoping matters, not just for test isolation: a rollback
/// triggered by *this* simulation's action must never bleed into a
/// different (e.g. later, or concurrently-tested) simulation's
/// authorization decisions.
///
/// A `Conservation` row whose `expires_at_tick` has passed is resolved
/// back to `Normal` here (logging that recovery as its own transition)
/// rather than reported stale -- callers never need to separately check
/// "but has it expired since."
pub async fn current_safety_state(
    pool: &PgPool,
    simulation_id: Option<Uuid>,
    current_tick: u64,
) -> Result<SafetyState, sqlx::Error> {
    struct Row {
        to_state: String,
        expires_at_tick: Option<i64>,
    }
    let row = match simulation_id {
        Some(sim_id) => {
            sqlx::query_as!(
                Row,
                r#"
                SELECT to_state, expires_at_tick FROM state_transitions
                WHERE simulation_id = $1 OR simulation_id IS NULL
                ORDER BY created_at DESC LIMIT 1
                "#,
                sim_id
            )
            .fetch_optional(pool)
            .await?
        }
        None => {
            sqlx::query_as!(
                Row,
                r#"
                SELECT to_state, expires_at_tick FROM state_transitions
                WHERE simulation_id IS NULL
                ORDER BY created_at DESC LIMIT 1
                "#
            )
            .fetch_optional(pool)
            .await?
        }
    };

    let Some(row) = row else {
        return Ok(SafetyState::Normal);
    };
    // Every row this module writes uses SafetyState::as_str(), so this
    // only fails if the database was written to by something else --
    // treat that as "assume the worst" rather than panicking.
    let state = SafetyState::parse(&row.to_state).unwrap_or(SafetyState::Stopped);

    if state == SafetyState::Conservation {
        if let Some(expires_at_tick) = row.expires_at_tick {
            if current_tick as i64 >= expires_at_tick {
                insert_state_transition(
                    pool,
                    simulation_id,
                    SafetyState::Conservation,
                    SafetyState::Normal,
                    &format!("conservation window expired at tick {expires_at_tick}"),
                    "orchestrator",
                    None,
                )
                .await?;
                return Ok(SafetyState::Normal);
            }
        }
    }

    Ok(state)
}

// --- Watchdog queries -----------------------------------------------------
//
// Both queries below are scoped to one simulation, not global across the
// whole `actions` table: a watchdog check must react to what *this*
// simulation's operator is doing, not to stale actions left over from a
// previous, unrelated simulation still sitting in the ledger (the same
// cross-simulation leak `current_safety_state` had to be fixed for).

/// The most recently started simulation -- what the watchdog checks
/// against when it isn't itself told which one is "current" (it isn't
/// connected to the bridge and has no other way to know).
pub async fn latest_simulation_id(pool: &PgPool) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!("SELECT id FROM simulations ORDER BY started_at DESC LIMIT 1")
        .fetch_optional(pool)
        .await
}

/// Actions authorized in the last minute for `simulation_id` -- the
/// watchdog's runaway-action circuit breaker
/// (`action_rate_stopped_threshold_per_minute`) counts against this, not
/// `max_actions_per_hour` (the governor's own, much gentler rate limit).
pub async fn actions_in_last_minute(
    pool: &PgPool,
    simulation_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM actions ac
        JOIN authorizations a ON a.id = ac.authorization_id
        JOIN proposals p ON p.id = a.proposal_id
        WHERE p.simulation_id = $1 AND ac.created_at >= now() - interval '1 minute'
        "#,
        simulation_id,
    )
    .fetch_one(pool)
    .await
}

pub struct RidePriceChange {
    pub ride_id: i64,
    pub price: i64,
    pub tick: i64,
}

/// Every `set_ride_price` action for `simulation_id` within `window_ticks`
/// of that simulation's most recent action, ordered by tick -- the
/// watchdog groups these by `ride_id` and feeds each ride's price
/// sequence to `crate::oscillation::count_reversals`. Windowed against
/// the simulation's own latest action tick (not some independently-
/// tracked "current tick" -- there is no such thing queryable from the
/// database: ticks only ever arrive on `heartbeat`, which isn't persisted
/// anywhere, so the actions under analysis are the only tick source this
/// check needs).
pub async fn recent_price_changes(
    pool: &PgPool,
    simulation_id: Uuid,
    window_ticks: u64,
) -> Result<Vec<RidePriceChange>, sqlx::Error> {
    sqlx::query_as!(
        RidePriceChange,
        r#"
        SELECT
            (ac.command->'params'->>'ride_id')::bigint AS "ride_id!",
            (ac.command->'params'->>'price')::bigint AS "price!",
            ac.tick AS "tick!"
        FROM actions ac
        JOIN authorizations a ON a.id = ac.authorization_id
        JOIN proposals p ON p.id = a.proposal_id
        WHERE p.simulation_id = $1
          AND ac.command->>'action' = 'set_ride_price'
          AND ac.tick >= (
              SELECT COALESCE(MAX(ac2.tick), 0)
              FROM actions ac2
              JOIN authorizations a2 ON a2.id = ac2.authorization_id
              JOIN proposals p2 ON p2.id = a2.proposal_id
              WHERE p2.simulation_id = $1
          ) - $2
        ORDER BY ac.tick ASC
        "#,
        simulation_id,
        window_ticks as i64,
    )
    .fetch_all(pool)
    .await
}
