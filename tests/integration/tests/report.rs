//! Exercises `orchestrator::report::generate` against a real Postgres
//! connection: a full proposal -> authorization -> action -> result
//! chain, a rollback, and a state transition all show up correctly in
//! the generated Markdown.

use governor::{Authorization, Decision, Proposal, SafetyState};
use orchestrator::db;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (see .env.example) to run the integration tests");
    db::connect(&database_url)
        .await
        .expect("connect to test database")
}

#[tokio::test]
async fn report_includes_ledger_rollback_and_state_transition() {
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    db::insert_simulation_start(&pool, simulation_id, "0.1.0", "0.5.3")
        .await
        .expect("insert simulation");

    let proposal = Proposal {
        agent: "The Operator".to_string(),
        assumptions: json!({ "rule": "queue_empty" }),
        predicted_effect: json!({ "guest_count": "expected to increase" }),
        confidence: 0.8,
        cost_envelope: json!({}),
        expiry_tick: 2000,
        ride_id: 0,
        proposed_price: 4,
    };
    let proposal_id = db::insert_proposal(&pool, simulation_id, &proposal)
        .await
        .expect("insert proposal");
    let authorization = Authorization {
        decision: Decision::Authorized,
        reason: "within budget, confidence, price bounds, and cooldown".to_string(),
        policy_version: "0.1.0".to_string(),
    };
    let authorization_id = db::insert_authorization(&pool, proposal_id, &authorization)
        .await
        .expect("insert authorization");
    let command = json!({ "action": "set_ride_price", "params": { "ride_id": 0, "price": 4 } });
    let action_id = db::insert_action(
        &pool,
        authorization_id,
        &command,
        &format!("report-test-{simulation_id}"),
        2000,
        100,
    )
    .await
    .expect("insert action");
    db::insert_action_result(&pool, action_id, Some(0), None)
        .await
        .expect("insert action_result");

    let snapshot_id = db::insert_snapshot(
        &pool,
        simulation_id,
        "autosave",
        "runtime/checkpoints/x.park",
        100,
    )
    .await
    .expect("insert snapshot");
    db::insert_rollback(&pool, simulation_id, snapshot_id, "test rollback", "manual")
        .await
        .expect("insert rollback");

    db::insert_state_transition(
        &pool,
        Some(simulation_id),
        SafetyState::Normal,
        SafetyState::Cautious,
        "test transition",
        "orchestrator",
        None,
    )
    .await
    .expect("insert state transition");

    let markdown = orchestrator::report::generate(&pool, simulation_id)
        .await
        .expect("generate report");

    assert!(markdown.contains(&simulation_id.to_string()));
    assert!(markdown.contains("The Operator"));
    assert!(markdown.contains("authorized"));
    assert!(markdown.contains("test rollback"));
    assert!(markdown.contains("test transition"));
    assert!(markdown.contains("None: every authorized action has a recorded result"));
}

#[tokio::test]
async fn report_errors_on_unknown_simulation() {
    let pool = pool().await;
    let result = orchestrator::report::generate(&pool, Uuid::now_v7()).await;
    assert!(result.is_err());
}
