use std::net::SocketAddr;
use std::path::Path;

use orchestrator::{config, db, new_shared, OrchestratorError, Persistence};

/// Health endpoint port. Not read from config/bridge.json (that file is
/// the bridge<->orchestrator port both sides must agree on); this is
/// orchestrator-only and has no other consumer yet, so a constant is
/// enough for 0.1.
const HEALTH_PORT: u16 = 8091;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bridge_config = config::load(Path::new("config/bridge.json"))?;
    let tcp_addr: SocketAddr = format!("{}:{}", bridge_config.host, bridge_config.port).parse()?;
    let health_addr: SocketAddr = format!("{}:{}", bridge_config.host, HEALTH_PORT).parse()?;

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| OrchestratorError::MissingEnvVar("DATABASE_URL".to_string()))?;
    let pool = db::connect(&database_url).await?;

    let shared = new_shared();
    let persistence = Persistence::spawn(pool, shared.clone());
    orchestrator::run(shared, persistence, tcp_addr, health_addr).await?;

    Ok(())
}
