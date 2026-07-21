//! A separate, small process that watches the orchestrator from the
//! outside: its own health endpoint, the database directly, the action
//! rate, and per-ride price oscillation. It only ever *escalates* the
//! safety state (toward Quarantine/Stopped) -- recovery back to Normal
//! either self-clears (Cautious, Conservation, both owned by the
//! orchestrator itself) or requires a human running `orchestrator
//! resolve`. See docs/DECISIONS.md ADR-0006.
//!
//! Deliberately has no dependency on the orchestrator's own in-process
//! state -- everything it reasons about is either the database directly
//! or the `/health` HTTP endpoint, so it keeps working (and keeps being
//! useful) even if the orchestrator process it's watching is completely
//! wedged or dead.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use governor::{Constitution, SafetyState};
use orchestrator::{config, db, oscillation, HEALTH_PORT};
use serde::Deserialize;
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct HealthResponse {
    state: String,
    db_state: String,
}

#[derive(Debug, thiserror::Error)]
enum HealthCheckError {
    #[error("connection failed: {0}")]
    Connect(#[from] std::io::Error),
    #[error("malformed health response: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("response had no body")]
    NoBody,
}

async fn get_health(addr: SocketAddr) -> Result<HealthResponse, HealthCheckError> {
    let mut stream = TcpStream::connect(addr).await?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    let body_start = response.find("\r\n\r\n").ok_or(HealthCheckError::NoBody)? + 4;
    Ok(serde_json::from_str(&response[body_start..])?)
}

/// Escalates toward `to` -- never downgrades (if already `Stopped`, the
/// most severe state, nothing the watchdog checks can make it worse) and
/// never fires redundantly (already in `to`). Uses `simulation_id: None`
/// -- see db.rs's `current_safety_state` doc comment on why a
/// watchdog-originated transition should apply globally rather than to
/// one specific simulation.
async fn escalate(pool: &PgPool, to: SafetyState, reason: String) {
    let from = match db::current_safety_state(pool, None, 0).await {
        Ok(state) => state,
        Err(err) => {
            tracing::error!(error = %err, "failed to read safety state before escalating");
            return;
        }
    };
    if from == to || from == SafetyState::Stopped {
        return;
    }
    match db::insert_state_transition(pool, None, from, to, &reason, "watchdog", None).await {
        Ok(id) => {
            tracing::warn!(%id, reason, from = from.as_str(), to = to.as_str(), "watchdog transition")
        }
        Err(err) => tracing::error!(error = %err, "failed to record watchdog transition"),
    }
}

async fn check_action_rate(pool: &PgPool, constitution: &Constitution, simulation_id: Uuid) {
    match db::actions_in_last_minute(pool, simulation_id).await {
        Ok(count) if count > constitution.action_rate_stopped_threshold_per_minute as i64 => {
            escalate(
                pool,
                SafetyState::Stopped,
                format!(
                    "{count} actions authorized in the last minute, past the threshold of {}",
                    constitution.action_rate_stopped_threshold_per_minute
                ),
            )
            .await;
        }
        Ok(_) => {}
        Err(err) => tracing::error!(error = %err, "failed to check action rate"),
    }
}

async fn check_oscillation(pool: &PgPool, constitution: &Constitution, simulation_id: Uuid) {
    let changes =
        match db::recent_price_changes(pool, simulation_id, constitution.oscillation_window_ticks)
            .await
        {
            Ok(changes) => changes,
            Err(err) => {
                tracing::error!(error = %err, "failed to check oscillation");
                return;
            }
        };

    let mut by_ride: HashMap<i64, Vec<i64>> = HashMap::new();
    for change in changes {
        by_ride
            .entry(change.ride_id)
            .or_default()
            .push(change.price);
    }

    for (ride_id, prices) in by_ride {
        let reversals = oscillation::count_reversals(&prices);
        if reversals > constitution.oscillation_max_reversals {
            escalate(
                pool,
                SafetyState::Quarantine,
                format!(
                    "ride {ride_id} price direction reversed {reversals} times in the last {} ticks, past the threshold of {}",
                    constitution.oscillation_window_ticks, constitution.oscillation_max_reversals
                ),
            )
            .await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| orchestrator::OrchestratorError::MissingEnvVar("DATABASE_URL".to_string()))?;
    let pool = db::connect(&database_url).await?;
    let constitution = Constitution::load(Path::new("config/constitution-0.1.yaml"))?;
    let bridge_config = config::load(Path::new("config/bridge.json"))?;
    let health_addr: SocketAddr = format!("{}:{}", bridge_config.host, HEALTH_PORT).parse()?;

    tracing::info!(
        %health_addr,
        poll_interval_secs = constitution.watchdog_poll_interval_secs,
        "watchdog starting"
    );

    let mut db_unreachable_since: Option<Instant> = None;

    loop {
        tokio::time::sleep(Duration::from_secs(
            constitution.watchdog_poll_interval_secs,
        ))
        .await;

        match get_health(health_addr).await {
            Ok(health) => {
                tracing::debug!(
                    bridge_state = health.state,
                    db_state = health.db_state,
                    "orchestrator health check ok"
                );
            }
            Err(err) => {
                tracing::warn!(error = %err, %health_addr, "orchestrator health endpoint unreachable");
            }
        }

        match sqlx::query("SELECT 1").execute(&pool).await {
            Ok(_) => {
                if let Some(since) = db_unreachable_since.take() {
                    let outage = since.elapsed();
                    tracing::info!(outage_secs = outage.as_secs(), "database reachable again");
                    if outage >= Duration::from_secs(constitution.db_unreachable_stopped_after_secs)
                    {
                        escalate(
                            &pool,
                            SafetyState::Stopped,
                            format!(
                                "database was unreachable for {}s, past the threshold of {}s",
                                outage.as_secs(),
                                constitution.db_unreachable_stopped_after_secs
                            ),
                        )
                        .await;
                    }
                }
            }
            Err(err) => {
                db_unreachable_since.get_or_insert_with(Instant::now);
                tracing::warn!(error = %err, "database unreachable");
                // Nothing else this loop iteration can do without a
                // database connection -- try again next interval.
                continue;
            }
        }

        match db::latest_simulation_id(&pool).await {
            Ok(Some(simulation_id)) => {
                check_action_rate(&pool, &constitution, simulation_id).await;
                check_oscillation(&pool, &constitution, simulation_id).await;
            }
            Ok(None) => {
                tracing::debug!("no simulation has connected yet; nothing to check");
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to look up the latest simulation");
            }
        }
    }
}
