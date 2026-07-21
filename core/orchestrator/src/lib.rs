pub mod config;
pub mod db;
pub mod error;
pub mod health;
pub mod operator;
pub mod oscillation;
pub mod snapshot;
pub mod startup;
pub mod state;
pub mod tcp_server;

pub use db::Persistence;
pub use error::OrchestratorError;
pub use snapshot::SnapshotConfig;
pub use startup::reconcile_on_startup;
pub use state::{new_shared, ConnectionState, DbState, Shared};

use std::net::SocketAddr;

use sqlx::PgPool;

/// Health endpoint port. Not read from config/bridge.json (that file is
/// the bridge<->orchestrator port both sides must agree on); this is
/// orchestrator-only, and shared with `src/bin/watchdog.rs` so both know
/// where to find it without duplicating the constant.
pub const HEALTH_PORT: u16 = 8091;

/// Runs the TCP server and health endpoint concurrently until either
/// exits (which, absent a bug, is never -- both loop forever). Returns
/// the first error either side hits.
pub async fn run(
    shared: Shared,
    persistence: Persistence,
    pool: PgPool,
    tcp_addr: SocketAddr,
    health_addr: SocketAddr,
    snapshot_config: SnapshotConfig,
) -> Result<(), OrchestratorError> {
    tokio::try_join!(
        tcp_server::run(shared.clone(), persistence, pool, tcp_addr, snapshot_config),
        health::run(shared, health_addr),
    )?;
    Ok(())
}
