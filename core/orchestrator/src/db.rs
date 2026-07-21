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
use governor::{Authorization, Decision, Proposal};
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

pub async fn connect(database_url: &str) -> Result<PgPool, OrchestratorError> {
    PgPoolOptions::new()
        .max_connections(5)
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

pub async fn insert_action(
    pool: &PgPool,
    authorization_id: Uuid,
    command: &Value,
    idempotency_key: &str,
    expiry_tick: u64,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO actions (authorization_id, command, idempotency_key, expiry_tick)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        authorization_id,
        command,
        idempotency_key,
        expiry_tick as i64,
    )
    .fetch_one(pool)
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
