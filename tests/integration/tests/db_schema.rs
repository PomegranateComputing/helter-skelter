//! Exercises the db/migrations/ schema directly (not through the
//! orchestrator's persistence worker): a full round-trip through every
//! table in the ledger chain, the idempotency_key UNIQUE constraint on
//! actions, and append-only enforcement.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    orchestrator::db::connect(&database_url)
        .await
        .expect("connect to test database")
}

#[tokio::test]
async fn full_ledger_chain_round_trips() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();

    sqlx::query!(
        "INSERT INTO simulations (id, bridge_version, openrct2_version) VALUES ($1, $2, $3)",
        simulation_id,
        "0.1.0",
        "0.5.3",
    )
    .execute(&pool)
    .await
    .expect("insert simulation");

    let sim_row = sqlx::query!(
        "SELECT id, bridge_version, openrct2_version, ended_at FROM simulations WHERE id = $1",
        simulation_id
    )
    .fetch_one(&pool)
    .await
    .expect("select simulation");
    assert_eq!(sim_row.id, simulation_id);
    assert_eq!(sim_row.bridge_version, "0.1.0");
    assert!(sim_row.ended_at.is_none());

    let message_id = Uuid::now_v7();
    let payload = json!({ "cash": 1000, "guest_count": 42, "park_rating": 700 });
    sqlx::query!(
        r#"
        INSERT INTO observations (simulation_id, message_id, recorded_at, payload, cash, guest_count, park_rating)
        VALUES ($1, $2, now(), $3, $4, $5, $6)
        "#,
        simulation_id,
        message_id,
        payload,
        1000_i64,
        42_i32,
        700_i32,
    )
    .execute(&pool)
    .await
    .expect("insert observation");

    let obs_row = sqlx::query!(
        "SELECT cash, guest_count, park_rating FROM observations WHERE message_id = $1",
        message_id
    )
    .fetch_one(&pool)
    .await
    .expect("select observation");
    assert_eq!(obs_row.cash, 1000);
    assert_eq!(obs_row.guest_count, 42);
    assert_eq!(obs_row.park_rating, 700);

    let proposal_id: Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO proposals (simulation_id, agent, assumptions, predicted_effect, confidence, cost_envelope, expiry_tick)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
        simulation_id,
        "The Operator",
        json!({ "queue_too_long": true }),
        json!({ "cash_delta": 10 }),
        0.8_f32,
        json!({ "max_price_delta": 1 }),
        1000_i64,
    )
    .fetch_one(&pool)
    .await
    .expect("insert proposal");

    let authorization_id: Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO authorizations (proposal_id, decision, reason, policy_version)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        proposal_id,
        "authorized",
        "within budget and confidence threshold",
        "0.1.0",
    )
    .fetch_one(&pool)
    .await
    .expect("insert authorization");

    let idempotency_key = format!("test-{}", Uuid::now_v7());
    let action_id: Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO actions (authorization_id, command, idempotency_key, expiry_tick)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        authorization_id,
        json!({ "action": "set_ride_price", "params": { "ride_id": 0, "price": 4 } }),
        idempotency_key,
        1000_i64,
    )
    .fetch_one(&pool)
    .await
    .expect("insert action");

    sqlx::query!(
        "INSERT INTO action_results (action_id, engine_cost, engine_error) VALUES ($1, $2, $3)",
        action_id,
        0_i64,
        None::<serde_json::Value>,
    )
    .execute(&pool)
    .await
    .expect("insert action_result");

    let result_row = sqlx::query!(
        "SELECT engine_cost, engine_error FROM action_results WHERE action_id = $1",
        action_id
    )
    .fetch_one(&pool)
    .await
    .expect("select action_result");
    assert_eq!(result_row.engine_cost, Some(0));
    assert!(result_row.engine_error.is_none());

    sqlx::query!(
        "INSERT INTO snapshots (simulation_id, kind, storage_path) VALUES ($1, $2, $3)",
        simulation_id,
        "manual",
        "runtime/checkpoints/test.park",
    )
    .execute(&pool)
    .await
    .expect("insert snapshot");

    let snapshot_row = sqlx::query!(
        "SELECT kind, storage_path FROM snapshots WHERE simulation_id = $1",
        simulation_id
    )
    .fetch_one(&pool)
    .await
    .expect("select snapshot");
    assert_eq!(snapshot_row.kind, "manual");
    assert_eq!(snapshot_row.storage_path, "runtime/checkpoints/test.park");
}

#[tokio::test]
async fn duplicate_idempotency_key_conflicts() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO simulations (id, bridge_version, openrct2_version) VALUES ($1, $2, $3)",
        simulation_id,
        "0.1.0",
        "0.5.3",
    )
    .execute(&pool)
    .await
    .expect("insert simulation");

    let proposal_id: Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO proposals (simulation_id, agent, assumptions, predicted_effect, confidence, cost_envelope, expiry_tick)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
        simulation_id,
        "The Operator",
        json!({}),
        json!({}),
        0.9_f32,
        json!({}),
        1000_i64,
    )
    .fetch_one(&pool)
    .await
    .expect("insert proposal");

    let authorization_id: Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO authorizations (proposal_id, decision, reason, policy_version)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        proposal_id,
        "authorized",
        "test",
        "0.1.0",
    )
    .fetch_one(&pool)
    .await
    .expect("insert authorization");

    let idempotency_key = format!("dup-{}", Uuid::now_v7());
    sqlx::query!(
        r#"
        INSERT INTO actions (authorization_id, command, idempotency_key, expiry_tick)
        VALUES ($1, $2, $3, $4)
        "#,
        authorization_id,
        json!({}),
        idempotency_key,
        1000_i64,
    )
    .execute(&pool)
    .await
    .expect("first insert with this idempotency_key must succeed");

    let second_insert = sqlx::query!(
        r#"
        INSERT INTO actions (authorization_id, command, idempotency_key, expiry_tick)
        VALUES ($1, $2, $3, $4)
        "#,
        authorization_id,
        json!({}),
        idempotency_key,
        1000_i64,
    )
    .execute(&pool)
    .await;

    let err = second_insert
        .expect_err("replaying the same idempotency_key must conflict, not silently insert");
    let db_err = err.as_database_error().expect("expected a database error");
    assert_eq!(
        db_err.code().as_deref(),
        Some("23505"),
        "expected a unique_violation (23505)"
    );
}

#[tokio::test]
async fn append_only_tables_reject_update_and_delete() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO simulations (id, bridge_version, openrct2_version) VALUES ($1, $2, $3)",
        simulation_id,
        "0.1.0",
        "0.5.3",
    )
    .execute(&pool)
    .await
    .expect("insert simulation");

    let message_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO observations (simulation_id, message_id, recorded_at, payload, cash, guest_count, park_rating)
        VALUES ($1, $2, now(), $3, $4, $5, $6)
        "#,
        simulation_id,
        message_id,
        json!({}),
        0_i64,
        0_i32,
        0_i32,
    )
    .execute(&pool)
    .await
    .expect("insert observation");

    let update_result = sqlx::query!(
        "UPDATE observations SET cash = 999 WHERE message_id = $1",
        message_id
    )
    .execute(&pool)
    .await;
    assert!(
        update_result.is_err(),
        "UPDATE on an append-only table must be rejected"
    );

    let delete_result = sqlx::query!("DELETE FROM observations WHERE message_id = $1", message_id)
        .execute(&pool)
        .await;
    assert!(
        delete_result.is_err(),
        "DELETE on an append-only table must be rejected"
    );
}
