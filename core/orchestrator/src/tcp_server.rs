use std::net::SocketAddr;

use common::protocol::{Envelope, Payload, PROTOCOL_VERSION};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::db::{PersistJob, Persistence};
use crate::error::{MessageRejection, OrchestratorError};
use crate::state::Shared;

/// Accepts bridge connections and serves them one at a time (0.1 expects a
/// single bridge, per docs/PROTOCOL.md: "one connection per running
/// simulation"). When a connection ends, goes back to accepting the next
/// one rather than exiting.
pub async fn run(
    shared: Shared,
    persistence: Persistence,
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

        if let Err(err) = handle_connection(&shared, &persistence, socket).await {
            tracing::warn!(%peer, error = %err, "connection ended with error");
        }

        {
            let mut state = shared.write().await;
            state.health.on_disconnect();
        }
        tracing::info!(%peer, "bridge disconnected");
    }
}

async fn handle_connection(
    shared: &Shared,
    persistence: &Persistence,
    socket: TcpStream,
) -> Result<(), OrchestratorError> {
    let mut lines = BufReader::new(socket).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        match parse_and_validate(&line) {
            Ok(envelope) => apply(shared, persistence, envelope).await,
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

async fn apply(shared: &Shared, persistence: &Persistence, envelope: Envelope) {
    let mut state = shared.write().await;
    match envelope.payload {
        Payload::Hello(hello) => {
            tracing::info!(role = ?hello.role, bridge_version = hello.bridge_version, "hello received");
            state.health.on_hello();
            persistence.submit(PersistJob::SimulationStart {
                simulation_id: envelope.simulation_id,
                bridge_version: hello.bridge_version,
                openrct2_version: hello.openrct2_version,
            });
        }
        Payload::Heartbeat(heartbeat) => {
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
                simulation_id: envelope.simulation_id,
                message_id: envelope.message_id,
                recorded_at: envelope.timestamp,
                payload: payload_json,
                cash: snapshot.cash,
                guest_count: snapshot.guest_count as i32,
                park_rating: snapshot.park_rating as i32,
            });
            state.world.record_snapshot(snapshot);
        }
        // command.request/result, shutdown, ack: not sent by the bridge in
        // this milestone (it only observes and transmits) and not yet
        // acted on by the orchestrator (no decision logic in this task).
        other => {
            tracing::debug!(kind = ?other, "received message with no handler in this milestone");
        }
    }
}
