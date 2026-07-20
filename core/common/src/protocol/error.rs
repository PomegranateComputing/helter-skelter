use crate::protocol::envelope::Kind;

/// Errors from constructing or validating a protocol [`Envelope`](crate::protocol::Envelope).
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("correlation_id is required for messages of kind {0:?}, but was null")]
    MissingCorrelationId(Kind),

    #[error("failed to (de)serialize protocol message: {0}")]
    Json(#[from] serde_json::Error),
}
