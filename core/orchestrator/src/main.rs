use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use governor::Constitution;
use orchestrator::snapshot::{self, SnapshotConfig, CURRENT_PARK_PATH};
use orchestrator::{config, db, new_shared, OrchestratorError, Persistence};
use uuid::Uuid;

/// Health endpoint port. Not read from config/bridge.json (that file is
/// the bridge<->orchestrator port both sides must agree on); this is
/// orchestrator-only and has no other consumer yet, so a constant is
/// enough for 0.1.
const HEALTH_PORT: u16 = 8091;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Restores a snapshot's park file to runtime/current-park.park (the
    /// park scripts/dev/run-stack.sh loads next) and records the
    /// rollback. Does not stop or restart a running stack itself -- see
    /// docs/DECISIONS.md ADR-0005 for why that's a separate, deliberate
    /// step, not something this subcommand can safely do on its own.
    Rollback {
        #[arg(long = "to")]
        to: Uuid,
        #[arg(long, default_value = "manual rollback via CLI")]
        reason: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| OrchestratorError::MissingEnvVar("DATABASE_URL".to_string()))?;
    let pool = db::connect(&database_url).await?;

    match cli.command {
        Some(Command::Rollback { to, reason }) => {
            let rollback_id = snapshot::restore_snapshot(
                &pool,
                to,
                Path::new(CURRENT_PARK_PATH),
                &reason,
                "manual",
            )
            .await?;
            println!(
                "rollback {rollback_id} recorded: snapshot {to} restored to {CURRENT_PARK_PATH}"
            );
            println!("stop the running stack (if any) and restart it to load the restored park");
            Ok(())
        }
        None => run_server(pool).await,
    }
}

async fn run_server(pool: sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let bridge_config = config::load(Path::new("config/bridge.json"))?;
    let tcp_addr: SocketAddr = format!("{}:{}", bridge_config.host, bridge_config.port).parse()?;
    let health_addr: SocketAddr = format!("{}:{}", bridge_config.host, HEALTH_PORT).parse()?;

    let constitution = Constitution::load(Path::new("config/constitution-0.1.yaml"))?;

    let snapshot_config = SnapshotConfig {
        script_path: PathBuf::from("scripts/dev/snapshot.sh"),
        checkpoint_root: PathBuf::from("runtime/checkpoints"),
    };

    let shared = new_shared(constitution);
    let persistence = Persistence::spawn(pool.clone(), shared.clone());
    orchestrator::run(
        shared,
        persistence,
        pool,
        tcp_addr,
        health_addr,
        snapshot_config,
    )
    .await?;

    Ok(())
}
