//! Feeds a controlled sequence of observation.snapshot envelopes (not the
//! static fixtures -- this needs specific queue_length/price values across
//! multiple snapshots to actually trigger the rule) to a real orchestrator
//! over a real TCP connection, acts as the bridge for the resulting
//! command.request, and asserts exactly which proposals/authorizations/
//! actions/action_results land in Postgres, correlated end to end.

use std::net::SocketAddr;
use std::time::Duration;

use chrono::Utc;
use orchestrator::{db, new_shared, Persistence, SnapshotConfig};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use uuid::Uuid;

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

async fn spawn_orchestrator() -> (SocketAddr, SocketAddr, sqlx::PgPool) {
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
            test_snapshot_config(),
        )
        .await
        .expect("orchestrator run");
    });

    sleep(Duration::from_millis(50)).await;
    (tcp_addr, health_addr, pool)
}

fn envelope(simulation_id: Uuid, kind: &str, payload: Value) -> Value {
    json!({
        "protocol_version": "0.1.0",
        "message_id": Uuid::now_v7().to_string(),
        "timestamp": Utc::now().to_rfc3339(),
        "simulation_id": simulation_id.to_string(),
        "correlation_id": null,
        "status": null,
        "error": null,
        "kind": kind,
        "payload": payload,
    })
}

fn snapshot_payload(ride_id: u32, queue_length: u32, price: u32) -> Value {
    json!({
        "park_date": { "year": 1, "month": 1, "day": 1 },
        "cash": 100000,
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

#[tokio::test]
async fn queue_too_long_produces_a_proposed_authorized_executed_price_change() {
    // Surfaces orchestrator-internal tracing (proposal/authorization/action
    // logs) if this test ever fails again -- cheap insurance after this
    // exact test caught a real FK-race bug during development.
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let (tcp_addr, _health_addr, db_pool) = spawn_orchestrator().await;
    let simulation_id = Uuid::now_v7();

    let mut stream = TcpStream::connect(tcp_addr).await.expect("bridge connects");
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "hello",
            json!({ "role": "bridge", "bridge_version": "0.1.0", "openrct2_version": "0.5.3" }),
        ),
    )
    .await;

    // Two consecutive high-queue snapshots for ride 0 (threshold is 5,
    // consecutive_snapshots_required is 2 in config/constitution-0.1.yaml)
    // -- the first alone must not be enough.
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            snapshot_payload(0, 10, 5),
        ),
    )
    .await;
    send(
        &mut write_half,
        &envelope(
            simulation_id,
            "observation.snapshot",
            snapshot_payload(0, 12, 5),
        ),
    )
    .await;

    // The orchestrator should now push a command.request for ride 0's
    // price increase (5 -> 6). Read it directly off the wire, as the
    // bridge would.
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timed out waiting for command.request")
        .expect("read command.request line");
    let command_request: Value = serde_json::from_str(&line).expect("parse command.request");
    assert_eq!(command_request["kind"], "command.request");
    assert_eq!(command_request["payload"]["action"], "set_ride_price");
    assert_eq!(command_request["payload"]["params"]["ride_id"], 0);
    assert_eq!(command_request["payload"]["params"]["price"], 6);
    let request_message_id = command_request["message_id"]
        .as_str()
        .expect("message_id")
        .to_string();

    // Act as the bridge: reply with a command.result correlated to the
    // request, as if the game action executed successfully.
    let result_envelope = json!({
        "protocol_version": "0.1.0",
        "message_id": Uuid::now_v7().to_string(),
        "timestamp": Utc::now().to_rfc3339(),
        "simulation_id": simulation_id.to_string(),
        "correlation_id": request_message_id,
        "status": "ok",
        "error": null,
        "kind": "command.result",
        "payload": { "engine_cost": 0, "engine_error": null },
    });
    send(&mut write_half, &result_envelope).await;

    // Give the orchestrator a moment to persist the action_result.
    sleep(Duration::from_millis(200)).await;

    // Exactly one proposal for this simulation, and it's the one expected.
    let proposals = sqlx::query!(
        "SELECT id, agent, confidence, expiry_tick FROM proposals WHERE simulation_id = $1",
        simulation_id
    )
    .fetch_all(&db_pool)
    .await
    .expect("query proposals");
    assert_eq!(proposals.len(), 1, "expected exactly one proposal");
    assert_eq!(proposals[0].agent, "The Operator");
    let proposal_id = proposals[0].id;

    // Exactly one authorization, and it's authorized.
    let authorizations = sqlx::query!(
        "SELECT id, decision, policy_version FROM authorizations WHERE proposal_id = $1",
        proposal_id
    )
    .fetch_all(&db_pool)
    .await
    .expect("query authorizations");
    assert_eq!(
        authorizations.len(),
        1,
        "expected exactly one authorization"
    );
    assert_eq!(authorizations[0].decision, "authorized");
    assert_eq!(authorizations[0].policy_version, "0.1.0");
    let authorization_id = authorizations[0].id;

    // Exactly one action, with the price-increase command and a non-empty
    // idempotency_key.
    let actions = sqlx::query!(
        "SELECT id, command, idempotency_key FROM actions WHERE authorization_id = $1",
        authorization_id
    )
    .fetch_all(&db_pool)
    .await
    .expect("query actions");
    assert_eq!(actions.len(), 1, "expected exactly one action");
    assert_eq!(actions[0].command["action"], "set_ride_price");
    assert_eq!(actions[0].command["params"]["price"], 6);
    assert!(!actions[0].idempotency_key.is_empty());
    let action_id = actions[0].id;

    // Exactly one action_result, matching the engine_cost we replied with.
    let results = sqlx::query!(
        "SELECT engine_cost, engine_error FROM action_results WHERE action_id = $1",
        action_id
    )
    .fetch_all(&db_pool)
    .await
    .expect("query action_results");
    assert_eq!(results.len(), 1, "expected exactly one action_result");
    assert_eq!(results[0].engine_cost, Some(0));
    assert!(results[0].engine_error.is_none());
}

#[tokio::test]
async fn low_confidence_never_reaches_governor_authorization_alone() {
    // Sanity check on the governor in isolation (not through the wire):
    // a proposal below min_confidence is rejected with a clear reason,
    // and never produces an action. Covered thoroughly by core/governor's
    // own table-driven unit tests; this just confirms the constitution
    // file the rest of this test module loads actually has a
    // min_confidence worth testing against.
    let constitution =
        governor::Constitution::load(&constitution_path()).expect("load constitution-0.1.yaml");
    assert!(constitution.min_confidence > 0.0);
}
