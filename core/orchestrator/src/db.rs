//! Durable persistence of observations and simulation lifecycle events.
//! Every write goes through [`Persistence`], a single background worker
//! fed by a bounded channel: if the database is unreachable, jobs queue
//! up (bounded, so a sustained outage can't grow memory unboundedly) and
//! retry with backoff, and connection handling continues uninterrupted --
//! a DB outage degrades the reported `db_state` to CAUTIOUS, it never
//! crashes the orchestrator or blocks the bridge connection.

use std::time::Duration;

use chrono::{DateTime, Utc};
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
    SimulationStart {
        simulation_id: Uuid,
        bridge_version: String,
        openrct2_version: String,
    },
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
        PersistJob::SimulationStart {
            simulation_id,
            bridge_version,
            openrct2_version,
        } => {
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
        }
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
