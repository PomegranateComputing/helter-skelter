//! Versioned message protocol shared by `bridge/openrct2-plugin` and
//! `core/orchestrator`. See `docs/PROTOCOL.md` for the transport and
//! versioning policy, and `bridge/protocol/` for the JSON Schema this
//! module's (de)serialization must stay compatible with.

mod commands;
mod envelope;
mod error;
mod messages;

pub use commands::{CommandAction, CommandRequest, StaffType};
pub use envelope::{Envelope, ErrorInfo, Kind, Payload, Status, PROTOCOL_VERSION};
pub use error::ProtocolError;
pub use messages::{
    Ack, CommandResult, Heartbeat, Hello, ObservationSnapshot, ParkDate, Ride, RideStatus, Role,
    Shutdown, ShutdownReason, StaffCounts, Weather,
};
