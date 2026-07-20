use std::net::SocketAddr;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::error::OrchestratorError;
use crate::state::{ConnectionState, DbState, Shared};

#[derive(Debug, Serialize)]
struct HealthResponse {
    state: ConnectionState,
    last_heartbeat_tick: Option<u64>,
    last_heartbeat_age_ms: Option<u128>,
    world_tick: Option<u64>,
    snapshots_recorded: usize,
    db_state: DbState,
}

async fn health(State(shared): State<Shared>) -> Json<HealthResponse> {
    let mut state = shared.write().await;
    state.health.tick_check();

    Json(HealthResponse {
        state: state.health.state,
        last_heartbeat_tick: state.health.last_heartbeat_tick,
        last_heartbeat_age_ms: state
            .health
            .last_heartbeat_at
            .map(|t| t.elapsed().as_millis()),
        world_tick: state.world.tick(),
        snapshots_recorded: state.world.history().len(),
        db_state: state.db_state,
    })
}

pub fn router(shared: Shared) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(shared)
}

pub async fn run(shared: Shared, addr: SocketAddr) -> Result<(), OrchestratorError> {
    tracing::info!(%addr, "health endpoint listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(shared)).await?;
    Ok(())
}
