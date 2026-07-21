//! Feeds a controlled sequence of observation.snapshot/command.result
//! envelopes to a real orchestrator over a real TCP connection (same
//! pattern as operator_slice.rs) and asserts the two automatic rollback
//! triggers from docs/DECISIONS.md ADR-0005: an engine_error on a
//! command.result, and a cash drop past max_unexpected_cash_drop on the
//! next observation.snapshot -- both must record a rollbacks row and put
//! the governor into conservation mode (verified by a subsequent
//! proposal being rejected, not silently skipped).

use std::net::SocketAddr;
use std::time::Duration;

use chrono::Utc;
use orchestrator::{db, new_shared, Persistence, SnapshotConfig};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

/// Serializes port allocation across this file's tests: each spawns an
/// orchestrator by probing an ephemeral port, dropping the probe listener,
/// then handing the bare port number to a task that binds it for real --
/// there's an inherent gap between "we know the port" and "it's actually
/// bound again," and with 3 tests in this file doing that concurrently,
/// CI (evidently faster/more prone to reusing a just-freed port than
/// local dev) hit the collision for real: `AddrInUse`. Holding this lock
/// across the whole probe+spawn+settle sequence in `spawn_orchestrator_with`
/// means no other test in this file starts probing until the previous
/// one's real bind has already succeeded.
static PORT_LOCK: Mutex<()> = Mutex::const_new(());

fn constitution_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/constitution-0.1.yaml")
}

/// No real OpenRCT2 process runs in these tests, so there's no real
/// autosave for scripts/dev/snapshot.sh to copy -- fake-snapshot.sh
/// stands in, writing an empty placeholder instead (see its doc comment).
fn test_snapshot_config() -> SnapshotConfig {
    SnapshotConfig {
        script_path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/fake-snapshot.sh"),
        checkpoint_root: std::env::temp_dir().join("helter-skelter-test-checkpoints"),
    }
}

async fn pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    db::connect(&database_url)
        .await
        .expect("connect to test database")
}

async fn spawn_orchestrator() -> (SocketAddr, sqlx::PgPool) {
    spawn_orchestrator_with(test_snapshot_config()).await
}

async fn spawn_orchestrator_with(snapshot_config: SnapshotConfig) -> (SocketAddr, sqlx::PgPool) {
    let _port_guard = PORT_LOCK.lock().await;

    let tcp_probe = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tcp probe");
    let tcp_addr = tcp_probe.local_addr().expect("tcp probe local_addr");
    drop(tcp_probe);

    let health_probe = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind health probe");
    let health_addr = health_probe.local_addr().expect("health probe local_addr");
    drop(health_probe);

    let constitution =
        governor::Constitution::load(&constitution_path()).expect("load constitution-0.1.yaml");
    let pool = pool().await;
    let shared = new_shared(constitution);
    let persistence = Persistence::spawn(pool.clone(), shared.clone());

    let run_pool = pool.clone();
    tokio::spawn(async move {
        orchestrator::run(
            shared,
            persistence,
            run_pool,
            tcp_addr,
            health_addr,
            snapshot_config,
        )
        .await
        .expect("orchestrator run");
    });

    sleep(Duration::from_millis(50)).await;
    (tcp_addr, pool)
}

fn envelope(
    simulation_id: Uuid,
    kind: &str,
    correlation_id: Option<Uuid>,
    payload: Value,
) -> Value {
    json!({
        "protocol_version": "0.1.0",
        "message_id": Uuid::now_v7().to_string(),
        "timestamp": Utc::now().to_rfc3339(),
        "simulation_id": simulation_id.to_string(),
        "correlation_id": correlation_id.map(|id| id.to_string()),
        "status": null,
        "error": null,
        "kind": kind,
        "payload": payload,
    })
}

fn snapshot_payload(ride_id: u32, queue_length: u32, price: u32, cash: i64) -> Value {
    json!({
        "park_date": { "year": 1, "month": 1, "day": 1 },
        "cash": cash,
        "loan": 0,
        "park_rating": 700,
        "guest_count": 100,
        "rides": [{
            "id": ride_id,
            "name": "Test Ride",
            "type": "rct2.ride.test",
            "status": "open",
            "price": price,
            "queue_length": queue_length,
            "downtime": 0,
        }],
        "staff_counts": { "handyman": 0, "mechanic": 0, "security": 0, "entertainer": 0 },
        "weather": "sunny",
    })
}

async fn send(stream: &mut (impl AsyncWriteExt + Unpin), value: &Value) {
    let line = serde_json::to_string(value).expect("serialize envelope");
    stream.write_all(line.as_bytes()).await.expect("write");
    stream.write_all(b"\n").await.expect("write newline");
}

/// Drives a ride through two consecutive high-queue snapshots (triggers
/// the price-increase rule, same fixture values as operator_slice.rs)
/// and returns the resulting command.request's message_id, to reply to.
async fn trigger_price_increase_and_read_request(
    write_half: &mut (impl AsyncWriteExt + Unpin),
    reader: &mut (impl AsyncBufReadExt + Unpin),
    simulation_id: Uuid,
    ride_id: u32,
    cash: i64,
) -> String {
    send(
        write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(ride_id, 10, 5, cash),
        ),
    )
    .await;
    send(
        write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(ride_id, 12, 5, cash),
        ),
    )
    .await;

    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timed out waiting for command.request")
        .expect("read command.request line");
    let command_request: Value = serde_json::from_str(&line).expect("parse command.request");
    assert_eq!(command_request["kind"], "command.request");
    assert_eq!(command_request["payload"]["params"]["ride_id"], ride_id);
    command_request["message_id"]
        .as_str()
        .expect("message_id")
        .to_string()
}

#[tokio::test]
async fn engine_error_triggers_automatic_rollback_and_blocks_the_next_proposal() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let (tcp_addr, db_pool) = spawn_orchestrator().await;
    let simulation_id = Uuid::now_v7();

    let mut stream = TcpStream::connect(tcp_addr).await.expect("bridge connects");
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "hello",
            None,
            json!({ "role": "bridge", "bridge_version": "0.1.0", "openrct2_version": "0.5.3" }),
        ),
    )
    .await;

    let request_message_id = trigger_price_increase_and_read_request(
        &mut write_half,
        &mut reader,
        simulation_id,
        0,
        100_000,
    )
    .await;

    // Reply as if the engine rejected the action.
    let result_envelope = envelope(
        simulation_id,
        "command.result",
        Some(request_message_id.parse().unwrap()),
        json!({ "engine_cost": null, "engine_error": { "code": "305", "message": "ride is closed" } }),
    );
    send(&mut write_half, &result_envelope).await;

    // Persisting the rollback happens asynchronously relative to this
    // send (process-spawn + DB round trips inside the orchestrator) --
    // poll rather than a fixed sleep, which flaked under CI's slower/more
    // contended runner.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let rollbacks = loop {
        let rows = sqlx::query!(
            "SELECT r.id, r.reason, r.triggered_by, r.snapshot_id, s.simulation_id \
             FROM rollbacks r JOIN snapshots s ON s.id = r.snapshot_id \
             WHERE s.simulation_id = $1",
            simulation_id
        )
        .fetch_all(&db_pool)
        .await
        .expect("query rollbacks");
        if !rows.is_empty() || tokio::time::Instant::now() >= deadline {
            break rows;
        }
        sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(rollbacks.len(), 1, "expected exactly one rollback");
    assert_eq!(rollbacks[0].triggered_by, "automatic");
    assert!(rollbacks[0].reason.contains("engine_error"));

    // Conservation mode should now reject a fresh proposal for a
    // *different* ride outright (ride 0 would also be blocked by its own
    // cooldown, which would confound the assertion -- ride 1 isolates
    // conservation specifically).
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(1, 10, 5, 100_000),
        ),
    )
    .await;
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(1, 12, 5, 100_000),
        ),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let ride1_authorizations = loop {
        let rows = sqlx::query!(
            r#"
            SELECT a.decision, a.reason FROM authorizations a
            JOIN proposals p ON p.id = a.proposal_id
            WHERE p.simulation_id = $1 AND p.assumptions->>'ride_id' = '1'
            "#,
            simulation_id
        )
        .fetch_all(&db_pool)
        .await
        .expect("query ride 1 authorizations");
        if !rows.is_empty() || tokio::time::Instant::now() >= deadline {
            break rows;
        }
        sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(ride1_authorizations.len(), 1);
    assert_eq!(ride1_authorizations[0].decision, "rejected");
    assert!(ride1_authorizations[0].reason.contains("conservation"));

    // No new action was sent for ride 1 -- only ride 0's original action
    // exists.
    let action_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM actions ac
        JOIN authorizations a ON a.id = ac.authorization_id
        JOIN proposals p ON p.id = a.proposal_id
        WHERE p.simulation_id = $1
        "#,
        simulation_id
    )
    .fetch_one(&db_pool)
    .await
    .expect("count actions");
    assert_eq!(action_count, Some(1));
}

#[tokio::test]
async fn wild_cash_drop_triggers_automatic_rollback() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let (tcp_addr, db_pool) = spawn_orchestrator().await;
    let simulation_id = Uuid::now_v7();

    let mut stream = TcpStream::connect(tcp_addr).await.expect("bridge connects");
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "hello",
            None,
            json!({ "role": "bridge", "bridge_version": "0.1.0", "openrct2_version": "0.5.3" }),
        ),
    )
    .await;

    let request_message_id = trigger_price_increase_and_read_request(
        &mut write_half,
        &mut reader,
        simulation_id,
        0,
        100_000,
    )
    .await;

    // The engine reports success...
    let result_envelope = envelope(
        simulation_id,
        "command.result",
        Some(request_message_id.parse().unwrap()),
        json!({ "engine_cost": 0, "engine_error": null }),
    );
    send(&mut write_half, &result_envelope).await;
    sleep(Duration::from_millis(100)).await;

    // ...but the next snapshot shows cash dropped by 2000, past
    // constitution-0.1.yaml's max_unexpected_cash_drop of 1000. Queue
    // length back to normal so this snapshot doesn't also trigger a new
    // price-change proposal and confound the assertion.
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 2, 6, 98_000),
        ),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let rollbacks = loop {
        let rows = sqlx::query!(
            "SELECT r.reason, r.triggered_by FROM rollbacks r \
             JOIN snapshots s ON s.id = r.snapshot_id \
             WHERE s.simulation_id = $1",
            simulation_id
        )
        .fetch_all(&db_pool)
        .await
        .expect("query rollbacks");
        if !rows.is_empty() || tokio::time::Instant::now() >= deadline {
            break rows;
        }
        sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(rollbacks.len(), 1, "expected exactly one rollback");
    assert_eq!(rollbacks[0].triggered_by, "automatic");
    assert!(rollbacks[0].reason.contains("cash dropped"));
}

/// Regression test for a real bug hit during phase 7's end-to-end proof
/// run: when no autosave exists yet, `ensure_recent_snapshot` fails --
/// that failure must be recorded as a rejected authorization *before*
/// `Governor::authorize` is ever called, not after, because `authorize`
/// commits per-ride cooldown/budget bookkeeping the moment it returns
/// Authorized. Getting the order wrong meant a snapshot failure silently
/// burned the ride's cooldown for an action that was never actually
/// executed -- the next otherwise-identical proposal would then be
/// rejected for "on cooldown" instead of getting a fair second attempt.
#[tokio::test]
async fn snapshot_failure_does_not_consume_the_ride_cooldown() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let failing_config = SnapshotConfig {
        script_path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/failing-snapshot.sh"),
        checkpoint_root: std::env::temp_dir().join("helter-skelter-test-checkpoints"),
    };
    let (tcp_addr, db_pool) = spawn_orchestrator_with(failing_config).await;
    let simulation_id = Uuid::now_v7();

    let mut stream = TcpStream::connect(tcp_addr).await.expect("bridge connects");
    let (_read_half, mut write_half) = stream.split();

    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "hello",
            None,
            json!({ "role": "bridge", "bridge_version": "0.1.0", "openrct2_version": "0.5.3" }),
        ),
    )
    .await;

    // First attempt: the rule fires, but the snapshot script always
    // fails -- must reject with "no recent snapshot", not authorize.
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 10, 5, 100_000),
        ),
    )
    .await;
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 12, 5, 100_000),
        ),
    )
    .await;

    // Two "normal" (neither high nor low) queue_length snapshots to fully
    // reset the rule's trailing window -- otherwise every subsequent
    // high-queue snapshot re-triggers the rule on its own (the window is
    // a rolling last-N-snapshots check, not a one-shot edge trigger), and
    // this test would see 3+ proposals instead of the clean 2 it needs.
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 2, 5, 100_000),
        ),
    )
    .await;
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 2, 5, 100_000),
        ),
    )
    .await;

    // Second attempt, same ride: if the bug were present, this would be
    // rejected for "cooldown" (proving the first attempt had wrongly
    // committed governor state despite never executing). With the fix,
    // it must fail the same way as the first attempt.
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 10, 5, 100_000),
        ),
    )
    .await;
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            None,
            snapshot_payload(0, 12, 5, 100_000),
        ),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let authorizations = loop {
        let rows = sqlx::query!(
            r#"
            SELECT a.decision, a.reason FROM authorizations a
            JOIN proposals p ON p.id = a.proposal_id
            WHERE p.simulation_id = $1
            ORDER BY a.created_at
            "#,
            simulation_id
        )
        .fetch_all(&db_pool)
        .await
        .expect("query authorizations");
        if rows.len() >= 2 || tokio::time::Instant::now() >= deadline {
            break rows;
        }
        sleep(Duration::from_millis(25)).await;
    };

    assert_eq!(
        authorizations.len(),
        2,
        "expected two rejected authorizations"
    );
    for auth in &authorizations {
        assert_eq!(auth.decision, "rejected");
        assert!(
            auth.reason.contains("no recent snapshot"),
            "expected a snapshot-failure reason, got: {}",
            auth.reason
        );
        assert!(
            !auth.reason.contains("cooldown"),
            "a snapshot failure must never be recorded as a cooldown rejection -- the ride was never actually authorized"
        );
    }

    // And no action was ever created, since nothing was ever authorized.
    let action_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM actions ac
        JOIN authorizations a ON a.id = ac.authorization_id
        JOIN proposals p ON p.id = a.proposal_id
        WHERE p.simulation_id = $1
        "#,
        simulation_id
    )
    .fetch_one(&db_pool)
    .await
    .expect("count actions");
    assert_eq!(action_count, Some(0));
}
