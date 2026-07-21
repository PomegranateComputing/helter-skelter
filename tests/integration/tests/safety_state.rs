//! Exercises the phase 8 safety-state machinery directly against a real
//! Postgres connection: `current_safety_state`'s simulation-scoping and
//! self-clearing Conservation expiry, `reconcile_on_startup`'s crash
//! recovery, and the watchdog's DB queries (`actions_in_last_minute`,
//! `recent_price_changes`). See docs/DECISIONS.md ADR-0006.

use governor::{Authorization, Decision, Proposal, SafetyState};
use orchestrator::db;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Serializes this file's tests: `current_safety_state`'s "fall back to
/// the latest global (simulation_id = NULL) transition" behavior is
/// correct in production (a real crash-recovery or watchdog event should
/// apply to whatever simulation comes next) but makes tests that write a
/// global transition race with *other* tests' scoped assertions when run
/// concurrently in the same process -- whichever test's global write
/// lands last wins the "ORDER BY created_at DESC" in every other test's
/// query too. Not a flaw in the production behavior, just not safe to
/// exercise in parallel against one shared table.
static TEST_LOCK: Mutex<()> = Mutex::const_new(());

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

/// Inserts a full proposal -> authorization -> action chain for
/// `ride_id`/`price`/`tick`, returning the action id -- the minimum
/// scaffolding an `actions` row's foreign keys require.
async fn insert_set_ride_price_action(
    pool: &PgPool,
    simulation_id: Uuid,
    ride_id: u32,
    price: i64,
    tick: u64,
) -> Uuid {
    let proposal = Proposal {
        agent: "The Operator".to_string(),
        assumptions: json!({}),
        predicted_effect: json!({}),
        confidence: 0.8,
        cost_envelope: json!({}),
        expiry_tick: tick + 2000,
        ride_id,
        proposed_price: price,
    };
    let proposal_id = db::insert_proposal(pool, simulation_id, &proposal)
        .await
        .expect("insert proposal");
    let authorization = Authorization {
        decision: Decision::Authorized,
        reason: "test".to_string(),
        policy_version: "test".to_string(),
    };
    let authorization_id = db::insert_authorization(pool, proposal_id, &authorization)
        .await
        .expect("insert authorization");
    let command =
        json!({ "action": "set_ride_price", "params": { "ride_id": ride_id, "price": price } });
    db::insert_action(
        pool,
        authorization_id,
        &command,
        &format!("test-{}", Uuid::now_v7()),
        tick + 2000,
        tick,
    )
    .await
    .expect("insert action")
}

// There's no test here for "a fresh simulation with zero transitions
// defaults to Normal" in isolation: other tests in this file legitimately
// write global (simulation_id = NULL) transitions, which -- correctly,
// by design -- future simulations see too (see
// current_safety_state_is_scoped_by_simulation_but_sees_global_transitions
// below), so that default is only observable in a database no test in
// this process has touched yet. It's exercised thoroughly regardless:
// every test in operator_slice.rs and rollback.rs that expects a
// proposal to be authorized is implicitly relying on it.

#[tokio::test]
async fn current_safety_state_is_scoped_by_simulation_but_sees_global_transitions() {
    let _guard = TEST_LOCK.lock().await;
    let pool = pool().await;
    let simulation_a = Uuid::now_v7();
    let simulation_b = Uuid::now_v7();
    insert_simulation(&pool, simulation_a).await;
    insert_simulation(&pool, simulation_b).await;

    // A global (simulation_id = NULL) transition, as crash-recovery or
    // the watchdog would record -- both simulations should see it.
    db::insert_state_transition(
        &pool,
        None,
        SafetyState::Normal,
        SafetyState::Cautious,
        "startup",
        "orchestrator",
        None,
    )
    .await
    .expect("insert global transition");

    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_a), 0)
            .await
            .unwrap(),
        SafetyState::Cautious
    );
    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_b), 0)
            .await
            .unwrap(),
        SafetyState::Cautious
    );

    // Simulation A alone rolls back and enters Conservation -- must not
    // affect simulation B's view of the current state.
    db::insert_state_transition(
        &pool,
        Some(simulation_a),
        SafetyState::Cautious,
        SafetyState::Conservation,
        "rollback",
        "orchestrator",
        Some(5000),
    )
    .await
    .expect("insert simulation-scoped transition");

    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_a), 100)
            .await
            .unwrap(),
        SafetyState::Conservation
    );
    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_b), 100)
            .await
            .unwrap(),
        SafetyState::Cautious,
        "simulation B must not see simulation A's rollback"
    );
}

#[tokio::test]
async fn conservation_self_clears_once_expires_at_tick_passes() {
    let _guard = TEST_LOCK.lock().await;
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    db::insert_state_transition(
        &pool,
        Some(simulation_id),
        SafetyState::Rollback,
        SafetyState::Conservation,
        "rollback",
        "orchestrator",
        Some(4000),
    )
    .await
    .expect("insert transition");

    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_id), 3999)
            .await
            .unwrap(),
        SafetyState::Conservation
    );
    assert_eq!(
        db::current_safety_state(&pool, Some(simulation_id), 4000)
            .await
            .unwrap(),
        SafetyState::Normal,
        "must self-clear once current_tick reaches expires_at_tick"
    );

    // The auto-recovery must itself be a logged transition, not a silent
    // reinterpretation -- every transition is a ledger event.
    let recovered = sqlx::query!(
        "SELECT from_state, to_state, triggered_by FROM state_transitions \
         WHERE simulation_id = $1 AND to_state = 'normal'",
        simulation_id
    )
    .fetch_one(&pool)
    .await
    .expect("query recovery transition");
    assert_eq!(recovered.from_state, "conservation");
    assert_eq!(recovered.triggered_by, "orchestrator");
}

#[tokio::test]
async fn reconcile_on_startup_enters_cautious_and_logs_in_flight_actions() {
    let _guard = TEST_LOCK.lock().await;
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    // An action with no action_result -- as if the process crashed after
    // sending the command.request but before the result arrived.
    let action_id = insert_set_ride_price_action(&pool, simulation_id, 0, 6, 100).await;

    let in_flight = db::find_in_flight_actions(&pool)
        .await
        .expect("find_in_flight_actions");
    assert!(in_flight.iter().any(|a| a.action_id == action_id));

    orchestrator::reconcile_on_startup(&pool)
        .await
        .expect("reconcile_on_startup");

    let state = db::current_safety_state(&pool, Some(simulation_id), 0)
        .await
        .expect("current_safety_state");
    assert_eq!(state, SafetyState::Cautious);

    // Resolving the in_flight action (recording its result) must remove
    // it from the in-flight set.
    db::insert_action_result(&pool, action_id, Some(0), None)
        .await
        .expect("insert action_result");
    let in_flight_after = db::find_in_flight_actions(&pool)
        .await
        .expect("find_in_flight_actions");
    assert!(!in_flight_after.iter().any(|a| a.action_id == action_id));
}

#[tokio::test]
async fn actions_in_last_minute_counts_only_recent_actions() {
    let _guard = TEST_LOCK.lock().await;
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    let before = db::actions_in_last_minute(&pool, simulation_id)
        .await
        .expect("actions_in_last_minute");
    assert_eq!(before, 0);

    insert_set_ride_price_action(&pool, simulation_id, 0, 6, 100).await;
    insert_set_ride_price_action(&pool, simulation_id, 1, 6, 100).await;

    let after = db::actions_in_last_minute(&pool, simulation_id)
        .await
        .expect("actions_in_last_minute");
    assert_eq!(after, 2);
}

#[tokio::test]
async fn recent_price_changes_windows_by_tick_and_feeds_oscillation_detection() {
    let _guard = TEST_LOCK.lock().await;
    let pool = pool().await;
    let simulation_id = Uuid::now_v7();
    insert_simulation(&pool, simulation_id).await;

    // A ride oscillating: 5 -> 6 -> 5 -> 6 within the window (3
    // reversals), plus an old, out-of-window change that must not count.
    insert_set_ride_price_action(&pool, simulation_id, 7, 1, 0).await; // far outside any reasonable window
    insert_set_ride_price_action(&pool, simulation_id, 7, 5, 10_000).await;
    insert_set_ride_price_action(&pool, simulation_id, 7, 6, 10_100).await;
    insert_set_ride_price_action(&pool, simulation_id, 7, 5, 10_200).await;
    insert_set_ride_price_action(&pool, simulation_id, 7, 6, 10_300).await;
    insert_set_ride_price_action(&pool, simulation_id, 7, 5, 10_400).await;

    let changes = db::recent_price_changes(&pool, simulation_id, 1000)
        .await
        .expect("recent_price_changes");
    let prices: Vec<i64> = changes
        .iter()
        .filter(|c| c.ride_id == 7)
        .map(|c| c.price)
        .collect();
    assert_eq!(
        prices,
        vec![5, 6, 5, 6, 5],
        "the tick=0 change must fall outside the 1000-tick window"
    );
    assert_eq!(orchestrator::oscillation::count_reversals(&prices), 3);
}
