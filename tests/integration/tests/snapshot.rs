//! Exercises core/orchestrator/src/snapshot.rs directly against a real
//! Postgres connection: `ensure_recent_snapshot`'s reuse-vs-refresh
//! decision, and `restore_snapshot`'s file copy + ledger recording. See
//! docs/DECISIONS.md ADR-0005 for why this is autosave-copying rather
//! than an on-demand save.

use std::path::Path;

use orchestrator::db;
use orchestrator::snapshot::{ensure_recent_snapshot, restore_snapshot, SnapshotConfig};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    db::connect(&database_url)
        .await
        .expect("connect to test database")
}

async fn insert_simulation(pool: &PgPool, simulation_id: Uuid) {
    db::insert_simulation_start(pool, simulation_id, "0.1.0", "0.5.3")
        .await
        .expect("insert simulation");
}

fn test_snapshot_config() -> SnapshotConfig {
    SnapshotConfig {
        script_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/fake-snapshot.sh"),
        checkpoint_root: std::env::temp_dir().join("helter-skelter-test-checkpoints"),
    }
}

#[tokio::test]
async fn ensure_recent_snapshot_creates_one_when_none_exists() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    let id = ensure_recent_snapshot(&pool, &test_snapshot_config(), simulation_id, 100, 2000)
        .await
        .expect("ensure_recent_snapshot");

    let (latest_id, tick) = db::latest_snapshot(&pool, simulation_id)
        .await
        .expect("query latest_snapshot")
        .expect("a snapshot was recorded");
    assert_eq!(latest_id, id);
    assert_eq!(tick, 100);
}

#[tokio::test]
async fn ensure_recent_snapshot_reuses_an_existing_fresh_one() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;
    let config = test_snapshot_config();

    let first = ensure_recent_snapshot(&pool, &config, simulation_id, 100, 2000)
        .await
        .expect("first ensure_recent_snapshot");
    // 500 ticks later, well within max_age_ticks=2000 -- must reuse, not
    // shell out to the script again.
    let second = ensure_recent_snapshot(&pool, &config, simulation_id, 600, 2000)
        .await
        .expect("second ensure_recent_snapshot");
    assert_eq!(first, second, "a fresh-enough snapshot must be reused");
}

#[tokio::test]
async fn ensure_recent_snapshot_refreshes_a_stale_one() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;
    let config = test_snapshot_config();

    let first = ensure_recent_snapshot(&pool, &config, simulation_id, 100, 2000)
        .await
        .expect("first ensure_recent_snapshot");
    // 3000 ticks later, past max_age_ticks=2000 -- must take a fresh one.
    let second = ensure_recent_snapshot(&pool, &config, simulation_id, 3100, 2000)
        .await
        .expect("second ensure_recent_snapshot");
    assert_ne!(first, second, "a stale snapshot must be refreshed");

    let (latest_id, tick) = db::latest_snapshot(&pool, simulation_id)
        .await
        .expect("query latest_snapshot")
        .expect("a snapshot was recorded");
    assert_eq!(latest_id, second);
    assert_eq!(tick, 3100);
}

#[tokio::test]
async fn restore_snapshot_copies_file_and_records_manual_rollback() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    let source_dir = std::env::temp_dir().join(format!("helter-skelter-test-{simulation_id}"));
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    let source_path = source_dir.join("42.park");
    std::fs::write(&source_path, b"fake park bytes").expect("write fake park");

    let snapshot_id = db::insert_snapshot(
        &pool,
        simulation_id,
        "autosave",
        source_path.to_str().unwrap(),
        42,
    )
    .await
    .expect("insert snapshot");

    let dest = source_dir.join("restored.park");
    let rollback_id = restore_snapshot(&pool, snapshot_id, &dest, "test rollback", "manual")
        .await
        .expect("restore_snapshot");

    let restored = std::fs::read(&dest).expect("read restored park");
    assert_eq!(restored, b"fake park bytes");

    let row = sqlx::query!(
        "SELECT snapshot_id, reason, triggered_by FROM rollbacks WHERE id = $1",
        rollback_id
    )
    .fetch_one(&pool)
    .await
    .expect("select rollback row");
    assert_eq!(row.snapshot_id, snapshot_id);
    assert_eq!(row.reason, "test rollback");
    assert_eq!(row.triggered_by, "manual");
}
