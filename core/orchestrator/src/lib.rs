pub mod config;
pub mod db;
pub mod error;
pub mod health;
pub mod operator;
pub mod state;
pub mod tcp_server;

pub use db::Persistence;
pub use error::OrchestratorError;
pub use state::{new_shared, ConnectionState, DbState, Shared};

use std::net::SocketAddr;

use sqlx::PgPool;

/// Runs the TCP server and health endpoint concurrently until either
/// exits (which, absent a bug, is never -- both loop forever). Returns
/// the first error either side hits.
pub async fn run(
    shared: Shared,
    persistence: Persistence,
    pool: PgPool,
    tcp_addr: SocketAddr,
    health_addr: SocketAddr,
) -> Result<(), OrchestratorError> {
    tokio::try_join!(
        tcp_server::run(shared.clone(), persistence, pool, tcp_addr),
        health::run(shared, health_addr),
    )?;
    Ok(())
}
