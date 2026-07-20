/// Typed errors for the orchestrator. Every fallible operation on a path
/// that runs during an unattended session returns one of these rather than
/// panicking -- see docs/CODING_STANDARD.md.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to read config at {path}: {source}")]
    ReadConfig {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config at {path}: {source}")]
    ParseConfig {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

/// A single rejected inbound line: malformed JSON, or a well-formed
/// envelope this orchestrator can't/won't act on (e.g. protocol version
/// mismatch). Never fatal to the connection -- logged and the line is
/// skipped, per docs/PROTOCOL.md's compatibility policy (a receiver must
/// never crash on an unrecognized/incompatible message).
#[derive(Debug, thiserror::Error)]
pub enum MessageRejection {
    #[error("malformed JSON: {0}")]
    Malformed(#[from] serde_json::Error),

    #[error("unsupported protocol_version: expected {expected}, got {actual}")]
    UnsupportedProtocolVersion {
        expected: &'static str,
        actual: String,
    },
}
