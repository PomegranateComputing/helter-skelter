use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;

use chrono::Utc;
use common::protocol::{CommandAction, CommandRequest, Envelope, Payload, PROTOCOL_VERSION};
use governor::{Constitution, Decision};
use serde_json::json;
use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use crate::db::{self, PersistJob, Persistence};
use crate::error::{MessageRejection, OrchestratorError};
use crate::operator;
use crate::snapshot::{self, SnapshotConfig, CURRENT_PARK_PATH};
use crate::state::Shared;

/// Accepts bridge connections and serves them one at a time (0.1 expects a
/// single bridge, per docs/PROTOCOL.md: "one connection per running
/// simulation"). When a connection ends, goes back to accepting the next
/// one rather than exiting.
pub async fn run(
    shared: Shared,
    persistence: Persistence,
    pool: PgPool,
    addr: SocketAddr,
    snapshot_config: SnapshotConfig,
) -> Result<(), OrchestratorError> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "tcp server listening");

    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!(%peer, "bridge connected");
        {
            let mut state = shared.write().await;
            state.health.on_connect();
        }

        if let Err(err) =
            handle_connection(&shared, &persistence, &pool, &snapshot_config, socket).await
        {
            tracing::warn!(%peer, error = %err, "connection ended with error");
        }

        {
            let mut state = shared.write().await;
            state.health.on_disconnect();
        }
        tracing::info!(%peer, "bridge disconnected");
    }
}

/// An authorized command.request awaiting its command.result, keyed by the
/// request's own message_id (the bridge must echo it back as the result's
/// correlation_id, per docs/PROTOCOL.md). `snapshot_id` and `cash_before`
/// carry forward to the post-execution checks: `snapshot_id` is what an
/// automatic rollback (triggered by either an engine_error here or a
/// wild cash delta once verified) restores to; `cash_before` is the
/// baseline that verification compares the next observation.snapshot's
/// cash against.
struct PendingAction {
    action_id: Uuid,
    snapshot_id: Uuid,
    cash_before: i64,
}

/// A successfully-executed action awaiting verification against the next
/// observation.snapshot's cash -- see `check_pending_verifications`.
struct PendingVerification {
    snapshot_id: Uuid,
    cash_before: i64,
}

async fn handle_connection(
    shared: &Shared,
    persistence: &Persistence,
    pool: &PgPool,
    snapshot_config: &SnapshotConfig,
    socket: TcpStream,
) -> Result<(), OrchestratorError> {
    let (read_half, mut write_half) = socket.into_split();
    let mut lines = BufReader::new(read_half).lines();
    let mut pending_actions: HashMap<Uuid, PendingAction> = HashMap::new();
    let mut pending_verifications: HashMap<Uuid, PendingVerification> = HashMap::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        match parse_and_validate(&line) {
            Ok(envelope) => {
                apply(
                    shared,
                    persistence,
                    pool,
                    snapshot_config,
                    &mut write_half,
                    &mut pending_actions,
                    &mut pending_verifications,
                    envelope,
                )
                .await
            }
            Err(rejection) => {
                tracing::warn!(error = %rejection, line, "rejected inbound message");
            }
        }
    }

    Ok(())
}

fn parse_and_validate(line: &str) -> Result<Envelope, MessageRejection> {
    let envelope: Envelope = serde_json::from_str(line)?;
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(MessageRejection::UnsupportedProtocolVersion {
            expected: PROTOCOL_VERSION,
            actual: envelope.protocol_version,
        });
    }
    Ok(envelope)
}

#[allow(clippy::too_many_arguments)]
async fn apply(
    shared: &Shared,
    persistence: &Persistence,
    pool: &PgPool,
    snapshot_config: &SnapshotConfig,
    writer: &mut OwnedWriteHalf,
    pending_actions: &mut HashMap<Uuid, PendingAction>,
    pending_verifications: &mut HashMap<Uuid, PendingVerification>,
    envelope: Envelope,
) {
    let simulation_id = envelope.simulation_id;
    match envelope.payload {
        Payload::Hello(hello) => {
            tracing::info!(role = ?hello.role, bridge_version = hello.bridge_version, "hello received");
            {
                let mut state = shared.write().await;
                state.health.on_hello();
            }
            // Synchronous, not persistence.submit(): everything the
            // decision pipeline persists foreign-keys to this row, so it
            // must be committed before an observation.snapshot arriving
            // right after hello can trigger a proposal -- see db.rs's
            // insert_simulation_start doc comment.
            if let Err(err) = db::insert_simulation_start(
                pool,
                simulation_id,
                &hello.bridge_version,
                &hello.openrct2_version,
            )
            .await
            {
                tracing::error!(error = %err, %simulation_id, "failed to persist simulation start");
            }
        }
        Payload::Heartbeat(heartbeat) => {
            let mut state = shared.write().await;
            state.health.on_heartbeat(heartbeat.tick);
            state.world.record_tick(heartbeat.tick);
        }
        Payload::ObservationSnapshot(snapshot) => {
            tracing::debug!(
                cash = snapshot.cash,
                guest_count = snapshot.guest_count,
                "observation.snapshot received"
            );
            let payload_json = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);
            persistence.submit(PersistJob::Observation {
                simulation_id,
                message_id: envelope.message_id,
                recorded_at: envelope.timestamp,
                payload: payload_json,
                cash: snapshot.cash,
                guest_count: snapshot.guest_count as i32,
                park_rating: snapshot.park_rating as i32,
            });

            // Hold the shared lock only long enough to record state and
            // copy out what the (lock-free) rule and DB calls below need --
            // never across an .await that talks to Postgres or the socket.
            let (history, constitution, current_tick, current_cash) = {
                let mut state = shared.write().await;
                state.world.record_snapshot(snapshot);
                let current_tick = state.world.tick().unwrap_or(0);
                let current_cash = state.world.history().back().map(|s| s.cash).unwrap_or(0);
                (
                    state.world.history().clone(),
                    state.governor.constitution().clone(),
                    current_tick,
                    current_cash,
                )
            };

            // Verify any action awaiting confirmation against *this*
            // snapshot's cash before considering new proposals -- an
            // unexpectedly bad outcome should gate new actions via
            // conservation mode, not race a fresh proposal on the same
            // tick.
            check_pending_verifications(
                shared,
                pool,
                pending_verifications,
                current_tick,
                current_cash,
                constitution.max_unexpected_cash_drop,
            )
            .await;

            let proposals = operator::propose_price_changes(&history, &constitution, current_tick);
            for proposal in proposals {
                handle_proposal(
                    shared,
                    pool,
                    snapshot_config,
                    writer,
                    pending_actions,
                    simulation_id,
                    current_tick,
                    current_cash,
                    &constitution,
                    proposal,
                )
                .await;
            }
        }
        Payload::CommandResult(result) => {
            let Some(correlation_id) = envelope.correlation_id else {
                // Schema-enforced mandatory for command.result; a bridge
                // sending one without it is itself a protocol violation,
                // not something to crash over.
                tracing::warn!("command.result missing correlation_id");
                return;
            };
            let Some(pending) = pending_actions.remove(&correlation_id) else {
                tracing::warn!(%correlation_id, "command.result for unknown or already-resolved action");
                return;
            };
            let engine_error = result
                .engine_error
                .as_ref()
                .map(|e| json!({ "code": e.code, "message": e.message }));
            if let Err(err) = db::insert_action_result(
                pool,
                pending.action_id,
                result.engine_cost,
                engine_error.clone(),
            )
            .await
            {
                tracing::error!(error = %err, action_id = %pending.action_id, "failed to persist action_result");
                return;
            }
            tracing::info!(action_id = %pending.action_id, engine_cost = ?result.engine_cost, "action_result persisted");

            match &result.engine_error {
                Some(e) => {
                    let current_tick = shared.read().await.world.tick().unwrap_or(0);
                    trigger_automatic_rollback(
                        shared,
                        pool,
                        pending.snapshot_id,
                        format!(
                            "engine_error on action {}: {} ({})",
                            pending.action_id, e.message, e.code
                        ),
                        current_tick,
                    )
                    .await;
                }
                None => {
                    pending_verifications.insert(
                        pending.action_id,
                        PendingVerification {
                            snapshot_id: pending.snapshot_id,
                            cash_before: pending.cash_before,
                        },
                    );
                }
            }
        }
        // shutdown, ack: not sent by the bridge in this milestone.
        other => {
            tracing::debug!(kind = ?other, "received message with no handler in this milestone");
        }
    }
}

/// Ensures a snapshot, authorizes, and, if authorized, executes one
/// proposal: persists the proposal and authorization regardless of the
/// outcome (a rejection -- including "no recent snapshot available" -- is
/// a fully-recorded outcome, not a silent no-op), and only builds+sends a
/// command.request if authorized. A failure to persist at any step means
/// "do not proceed" -- see db.rs's module docs on why the decision
/// pipeline is synchronous rather than buffered like observations.
#[allow(clippy::too_many_arguments)]
async fn handle_proposal(
    shared: &Shared,
    pool: &PgPool,
    snapshot_config: &SnapshotConfig,
    writer: &mut OwnedWriteHalf,
    pending_actions: &mut HashMap<Uuid, PendingAction>,
    simulation_id: Uuid,
    current_tick: u64,
    current_cash: i64,
    constitution: &Constitution,
    proposal: governor::Proposal,
) {
    let proposal_id = match db::insert_proposal(pool, simulation_id, &proposal).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, ride_id = proposal.ride_id, "failed to persist proposal; not authorizing/executing");
            return;
        }
    };

    // Ensure a snapshot recent enough to roll back to exists *before*
    // consulting the governor, not after: `Governor::authorize` has side
    // effects (it commits the rate-limit/cooldown bookkeeping the moment
    // it returns Authorized), and a snapshot failure discovered afterward
    // would silently burn that ride's cooldown for an action that never
    // actually executed -- a real bug this exact ordering hit during the
    // real end-to-end proof run (see docs/DECISIONS.md ADR-0005). A
    // missing snapshot is recorded as a rejected authorization, same as
    // any other reason the governor might say no.
    let snapshot_id = match snapshot::ensure_recent_snapshot(
        pool,
        snapshot_config,
        simulation_id,
        current_tick,
        constitution.snapshot_max_age_ticks,
    )
    .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, %proposal_id, "failed to ensure a recent snapshot; not authorizing/executing");
            let authorization = governor::Authorization {
                decision: Decision::Rejected,
                reason: format!("no recent snapshot available: {err}"),
                policy_version: constitution.policy_version.clone(),
            };
            if let Err(err) = db::insert_authorization(pool, proposal_id, &authorization).await {
                tracing::error!(error = %err, %proposal_id, "failed to persist authorization");
            }
            return;
        }
    };

    let authorization = {
        let mut state = shared.write().await;
        state.governor.authorize(&proposal, current_tick)
    };

    let authorization_id = match db::insert_authorization(pool, proposal_id, &authorization).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, %proposal_id, "failed to persist authorization; not executing");
            return;
        }
    };

    if authorization.decision != Decision::Authorized {
        tracing::info!(reason = %authorization.reason, ride_id = proposal.ride_id, "proposal rejected");
        return;
    }

    // Scoped by simulation_id, not just ride_id+tick: the latter repeats
    // across different simulations (e.g. tick defaults to 0 until the
    // first heartbeat), and idempotency_key is globally UNIQUE in the
    // schema -- a real bug this exact line caused during development,
    // surfaced by re-running the integration tests against a database
    // that already had a prior run's rows in it.
    let idempotency_key = format!(
        "operator-sim{simulation_id}-ride{}-tick{current_tick}",
        proposal.ride_id
    );
    let command = json!({
        "action": "set_ride_price",
        "params": { "ride_id": proposal.ride_id, "price": proposal.proposed_price }
    });
    let action_id = match db::insert_action(
        pool,
        authorization_id,
        &command,
        &idempotency_key,
        proposal.expiry_tick,
    )
    .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, %authorization_id, "failed to persist action; not sending command.request");
            return;
        }
    };

    let message_id = Uuid::now_v7();
    let outbound = Envelope {
        protocol_version: PROTOCOL_VERSION.to_string(),
        message_id,
        timestamp: Utc::now(),
        simulation_id,
        payload: Payload::CommandRequest(CommandRequest {
            action: CommandAction::SetRidePrice {
                ride_id: proposal.ride_id,
                price: proposal.proposed_price as u32,
            },
            idempotency_key,
            expiry_tick: proposal.expiry_tick,
        }),
        correlation_id: None,
        status: None,
        error: None,
    };

    let line = match serde_json::to_string(&outbound) {
        Ok(line) => line,
        Err(err) => {
            tracing::error!(error = %err, %action_id, "failed to serialize command.request");
            return;
        }
    };

    if let Err(err) = writer.write_all(format!("{line}\n").as_bytes()).await {
        tracing::warn!(error = %err, %action_id, "failed to send command.request to bridge");
        return;
    }

    pending_actions.insert(
        message_id,
        PendingAction {
            action_id,
            snapshot_id,
            cash_before: current_cash,
        },
    );
    tracing::info!(
        ride_id = proposal.ride_id,
        price = proposal.proposed_price,
        %action_id,
        %snapshot_id,
        "command.request sent"
    );
}

/// Checks every action awaiting verification against the cash this
/// snapshot just recorded, one shot each -- an action either verifies
/// clean or triggers a rollback the moment its first post-execution
/// snapshot looks wrong; there's no repeated re-checking across several
/// snapshots.
async fn check_pending_verifications(
    shared: &Shared,
    pool: &PgPool,
    pending_verifications: &mut HashMap<Uuid, PendingVerification>,
    current_tick: u64,
    current_cash: i64,
    max_unexpected_cash_drop: i64,
) {
    for (action_id, pending) in pending_verifications.drain() {
        let delta = current_cash - pending.cash_before;
        if delta < -max_unexpected_cash_drop {
            tracing::warn!(
                %action_id,
                delta,
                max_unexpected_cash_drop,
                "cash dropped more than max_unexpected_cash_drop after an authorized action"
            );
            trigger_automatic_rollback(
                shared,
                pool,
                pending.snapshot_id,
                format!(
                    "cash dropped by {} after action {action_id} (limit {max_unexpected_cash_drop})",
                    -delta
                ),
                current_tick,
            )
            .await;
        }
    }
}

/// Records a rollback event referencing `snapshot_id` (restoring its file
/// to `runtime/current-park.park`, the park the next `openrct2-cli` start
/// loads -- see docs/DECISIONS.md ADR-0005 on why this can't hot-swap the
/// *running* engine's state) and enters conservation mode so no further
/// proposal is authorized for a while. A failure to even record the
/// rollback is logged loudly -- there is no further fallback below this.
async fn trigger_automatic_rollback(
    shared: &Shared,
    pool: &PgPool,
    snapshot_id: Uuid,
    reason: String,
    current_tick: u64,
) {
    let dest = Path::new(CURRENT_PARK_PATH);
    match snapshot::restore_snapshot(pool, snapshot_id, dest, &reason, "automatic").await {
        Ok(rollback_id) => {
            let mut state = shared.write().await;
            state.governor.enter_conservation(current_tick);
            tracing::warn!(
                %rollback_id,
                %snapshot_id,
                reason,
                mode = ?state.governor.mode(),
                "automatic rollback recorded; entering conservation mode"
            );
        }
        Err(err) => {
            tracing::error!(error = %err, %snapshot_id, reason, "failed to record automatic rollback");
        }
    }
}
