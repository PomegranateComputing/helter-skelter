use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::protocol::commands::CommandRequest;
use crate::protocol::error::ProtocolError;
use crate::protocol::messages::{
    Ack, CommandResult, Heartbeat, Hello, ObservationSnapshot, Shutdown,
};

/// The only protocol version this crate currently emits or accepts.
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Wire-format-free identifier for [`Payload`]'s variants, used in error messages and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hello,
    Heartbeat,
    ObservationSnapshot,
    CommandRequest,
    CommandResult,
    Shutdown,
    Ack,
}

/// `status` on an envelope that carries a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Error,
}

/// A structured, non-free-form error, used both at the envelope level and inside
/// `command.result` payloads (as `engine_error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

/// The `kind` + `payload` pair. Serde's adjacently-tagged representation
/// (`tag = "kind"`, `content = "payload"`) makes it structurally impossible to
/// construct a message whose `kind` and payload type disagree, and serializes to
/// exactly the wire shape described by `bridge/protocol/envelope.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum Payload {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "heartbeat")]
    Heartbeat(Heartbeat),
    #[serde(rename = "observation.snapshot")]
    ObservationSnapshot(ObservationSnapshot),
    #[serde(rename = "command.request")]
    CommandRequest(CommandRequest),
    #[serde(rename = "command.result")]
    CommandResult(CommandResult),
    #[serde(rename = "shutdown")]
    Shutdown(Shutdown),
    #[serde(rename = "ack")]
    Ack(Ack),
}

impl Payload {
    pub fn kind(&self) -> Kind {
        match self {
            Payload::Hello(_) => Kind::Hello,
            Payload::Heartbeat(_) => Kind::Heartbeat,
            Payload::ObservationSnapshot(_) => Kind::ObservationSnapshot,
            Payload::CommandRequest(_) => Kind::CommandRequest,
            Payload::CommandResult(_) => Kind::CommandResult,
            Payload::Shutdown(_) => Kind::Shutdown,
            Payload::Ack(_) => Kind::Ack,
        }
    }

    /// True for kinds that must carry a non-null `correlation_id`
    /// (see `bridge/protocol/envelope.schema.json`'s `allOf`).
    fn requires_correlation_id(&self) -> bool {
        matches!(self.kind(), Kind::CommandResult | Kind::Ack)
    }
}

/// Top-level wire message. See `docs/PROTOCOL.md` and
/// `bridge/protocol/envelope.schema.json` for the authoritative shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: String,
    pub message_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub simulation_id: Uuid,
    #[serde(flatten)]
    pub payload: Payload,
    pub correlation_id: Option<Uuid>,
    pub status: Option<Status>,
    pub error: Option<ErrorInfo>,
}

impl Envelope {
    /// Builds a new envelope, generating `message_id` (UUIDv7) and `timestamp` (now),
    /// and enforcing the correlation_id-mandatory-for-some-kinds rule before returning.
    pub fn new(
        simulation_id: Uuid,
        payload: Payload,
        correlation_id: Option<Uuid>,
        status: Option<Status>,
        error: Option<ErrorInfo>,
    ) -> Result<Self, ProtocolError> {
        let envelope = Envelope {
            protocol_version: PROTOCOL_VERSION.to_string(),
            message_id: Uuid::now_v7(),
            timestamp: Utc::now(),
            simulation_id,
            payload,
            correlation_id,
            status,
            error,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Re-checks the rules that the type system alone can't express, notably that
    /// `command.result` and `ack` messages carry a non-null `correlation_id`.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.payload.requires_correlation_id() && self.correlation_id.is_none() {
            return Err(ProtocolError::MissingCorrelationId(self.payload.kind()));
        }
        Ok(())
    }
}
