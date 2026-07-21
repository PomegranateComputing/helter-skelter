use std::collections::HashMap;
use std::net::SocketAddr;

use chrono::Utc;
use common::protocol::{CommandAction, CommandRequest, Envelope, Payload, PROTOCOL_VERSION};
use governor::Decision;
use serde_json::json;
use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use crate::db::{self, PersistJob, Persistence};
use crate::error::{MessageRejection, OrchestratorError};
use crate::operator;
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

        if let Err(err) = handle_connection(&shared, &persistence, &pool, socket).await {
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
/// correlation_id, per docs/PROTOCOL.md).
struct PendingAction {
    action_id: Uuid,
}

async fn handle_connection(
    shared: &Shared,
    persistence: &Persistence,
    pool: &PgPool,
    socket: TcpStream,
) -> Result<(), OrchestratorError> {
    let (read_half, mut write_half) = socket.into_split();
    let mut lines = BufReader::new(read_half).lines();
    let mut pending_actions: HashMap<Uuid, PendingAction> = HashMap::new();

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
                    &mut write_half,
                    &mut pending_actions,
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

async fn apply(
    shared: &Shared,
    persistence: &Persistence,
    pool: &PgPool,
    writer: &mut OwnedWriteHalf,
    pending_actions: &mut HashMap<Uuid, PendingAction>,
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
            let (history, constitution, current_tick) = {
                let mut state = shared.write().await;
                state.world.record_snapshot(snapshot);
                let current_tick = state.world.tick().unwrap_or(0);
                (
                    state.world.history().clone(),
                    state.governor.constitution().clone(),
                    current_tick,
                )
            };

            let proposals = operator::propose_price_changes(&history, &constitution, current_tick);
            for proposal in proposals {
                handle_proposal(
                    shared,
                    pool,
                    writer,
                    pending_actions,
                    simulation_id,
                    current_tick,
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
            if let Err(err) =
                db::insert_action_result(pool, pending.action_id, result.engine_cost, engine_error)
                    .await
            {
                tracing::error!(error = %err, action_id = %pending.action_id, "failed to persist action_result");
            } else {
                tracing::info!(action_id = %pending.action_id, engine_cost = ?result.engine_cost, "action_result persisted");
            }
        }
        // shutdown, ack: not sent by the bridge in this milestone.
        other => {
            tracing::debug!(kind = ?other, "received message with no handler in this milestone");
        }
    }
}

/// Authorizes and, if authorized, executes one proposal: persists the
/// proposal and authorization regardless of the decision (a rejection is
/// a fully-recorded outcome, not a silent no-op), and only builds+sends a
/// command.request if authorized. A failure to persist at any step means
/// "do not proceed" -- see db.rs's module docs on why the decision
/// pipeline is synchronous rather than buffered like observations.
#[allow(clippy::too_many_arguments)]
async fn handle_proposal(
    shared: &Shared,
    pool: &PgPool,
    writer: &mut OwnedWriteHalf,
    pending_actions: &mut HashMap<Uuid, PendingAction>,
    simulation_id: Uuid,
    current_tick: u64,
    proposal: governor::Proposal,
) {
    let authorization = {
        let mut state = shared.write().await;
        state.governor.authorize(&proposal, current_tick)
    };

    let proposal_id = match db::insert_proposal(pool, simulation_id, &proposal).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, ride_id = proposal.ride_id, "failed to persist proposal; not authorizing/executing");
            return;
        }
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

    pending_actions.insert(message_id, PendingAction { action_id });
    tracing::info!(
        ride_id = proposal.ride_id,
        price = proposal.proposed_price,
        %action_id,
        "command.request sent"
    );
}
