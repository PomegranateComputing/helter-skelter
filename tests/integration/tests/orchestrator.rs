//! Spawns a real orchestrator (TCP server + health endpoint), feeds it the
//! canonical fixtures from bridge/messages/fixtures/ over a real TCP
//! connection (as the bridge would), and asserts the connection health
//! state machine and /health endpoint behave correctly end to end.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use orchestrator::{db, new_shared, Persistence};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bridge/messages/fixtures")
}

/// Fixtures on disk are pretty-printed (multi-line) for human readability,
/// but the wire format is NDJSON -- one compact JSON object per line. This
/// re-serializes to a single line, as the real bridge's `JSON.stringify`
/// would produce.
fn read_fixture_as_ndjson_line(name: &str) -> String {
    let text = std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("reading {name}: {e}"));
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {name}: {e}"));
    serde_json::to_string(&value).unwrap_or_else(|e| panic!("re-serializing {name}: {e}"))
}

async fn send_fixture(bridge: &mut TcpStream, name: &str) {
    let line = read_fixture_as_ndjson_line(name);
    bridge.write_all(line.as_bytes()).await.unwrap();
    bridge.write_all(b"\n").await.unwrap();
}

async fn pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    db::connect(&database_url)
        .await
        .expect("connect to test database")
}

struct ObservedRow {
    cash: i64,
    guest_count: i32,
    park_rating: i32,
}

/// Polls for the observations row written by the orchestrator's
/// persistence worker, which runs asynchronously relative to the TCP
/// message that triggered it.
async fn wait_for_row(
    pool: &sqlx::PgPool,
    message_id: uuid::Uuid,
    timeout: Duration,
) -> ObservedRow {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let row = sqlx::query!(
            "SELECT cash, guest_count, park_rating FROM observations WHERE message_id = $1",
            message_id
        )
        .fetch_optional(pool)
        .await
        .expect("query observations");
        if let Some(row) = row {
            return ObservedRow {
                cash: row.cash,
                guest_count: row.guest_count,
                park_rating: row.park_rating,
            };
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("observation with message_id {message_id} was not persisted within {timeout:?}");
        }
        sleep(Duration::from_millis(25)).await;
    }
}

/// Binds two ephemeral ports, drops the listeners, and starts the real
/// orchestrator on those exact addresses -- avoids hardcoding a port that
/// could collide across parallel test runs.
async fn spawn_orchestrator() -> (SocketAddr, SocketAddr) {
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

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    let pool = db::connect(&database_url)
        .await
        .expect("connect to test database");

    let shared = new_shared();
    let persistence = Persistence::spawn(pool, shared.clone());
    tokio::spawn(async move {
        orchestrator::run(shared, persistence, tcp_addr, health_addr)
            .await
            .expect("orchestrator run");
    });

    // Give the listeners a moment to actually bind before the test connects.
    sleep(Duration::from_millis(50)).await;
    (tcp_addr, health_addr)
}

async fn get_health(health_addr: SocketAddr) -> serde_json::Value {
    let mut stream = TcpStream::connect(health_addr)
        .await
        .expect("connect to health endpoint");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write health request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read health response");

    let body_start = response
        .find("\r\n\r\n")
        .expect("HTTP response has a header/body separator")
        + 4;
    serde_json::from_str(&response[body_start..]).expect("health response body is valid JSON")
}

/// Polls `get_health` until `predicate` matches or `timeout` elapses,
/// returning the last observed value either way. Avoids fixed sleeps for
/// asynchronous state transitions that don't happen synchronously with
/// the write that triggers them (e.g. disconnect detection).
async fn wait_for_health(
    health_addr: SocketAddr,
    timeout: Duration,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let value = get_health(health_addr).await;
        if predicate(&value) || tokio::time::Instant::now() >= deadline {
            return value;
        }
        sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn full_connection_lifecycle_over_real_tcp() {
    let (tcp_addr, health_addr) = spawn_orchestrator().await;

    // Before any bridge connects: CONNECTING.
    let health = get_health(health_addr).await;
    assert_eq!(health["state"], "connecting");

    let mut bridge = TcpStream::connect(tcp_addr)
        .await
        .expect("bridge connects to orchestrator");

    // hello -> LIVE.
    send_fixture(&mut bridge, "hello.json").await;
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["state"] == "live"
    })
    .await;
    assert_eq!(health["state"], "live");

    // heartbeat { tick: 12345 } -> last_heartbeat_tick recorded.
    send_fixture(&mut bridge, "heartbeat.json").await;
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["last_heartbeat_tick"] == 12345
    })
    .await;
    assert_eq!(health["last_heartbeat_tick"], 12345);
    assert_eq!(health["state"], "live");

    // observation.snapshot -> recorded in world-model AND persisted to
    // Postgres by the orchestrator's own persistence worker (not just
    // schema-level correctness, which db_schema.rs covers separately).
    send_fixture(&mut bridge, "observation_snapshot.json").await;
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["snapshots_recorded"] == 1
    })
    .await;
    assert_eq!(health["snapshots_recorded"], 1);
    assert_eq!(health["db_state"], "connected");

    let db_pool = pool().await;
    let fixture_message_id: uuid::Uuid = "019f80c2-e97d-7802-af54-1970b03ff16b".parse().unwrap();
    let observed = wait_for_row(&db_pool, fixture_message_id, Duration::from_secs(2)).await;
    assert_eq!(observed.cash, 154302);
    assert_eq!(observed.guest_count, 483);
    assert_eq!(observed.park_rating, 742);

    // A malformed line must not crash the connection or the orchestrator --
    // the next well-formed message must still be processed.
    bridge
        .write_all(b"{ this is not valid json\n")
        .await
        .unwrap();
    send_fixture(&mut bridge, "heartbeat.json").await; // same tick, just proving liveness survives
    sleep(Duration::from_millis(100)).await;
    let health = get_health(health_addr).await;
    assert_eq!(
        health["state"], "live",
        "malformed line must not break the connection"
    );

    // Disconnect -> LOST.
    drop(bridge);
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["state"] == "lost"
    })
    .await;
    assert_eq!(health["state"], "lost");

    // A fresh connection resets to CONNECTING, then LIVE again on hello.
    let mut bridge2 = TcpStream::connect(tcp_addr)
        .await
        .expect("second bridge connection");
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["state"] == "connecting"
    })
    .await;
    assert_eq!(health["state"], "connecting");

    send_fixture(&mut bridge2, "hello.json").await;
    let health = wait_for_health(health_addr, Duration::from_secs(2), |h| {
        h["state"] == "live"
    })
    .await;
    assert_eq!(health["state"], "live");
}
