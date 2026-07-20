use serde::{Deserialize, Serialize};

use crate::protocol::envelope::ErrorInfo;

/// `hello` payload: the first message on a fresh connection, either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Bridge,
    Orchestrator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub role: Role,
    pub bridge_version: String,
    pub openrct2_version: String,
}

/// `heartbeat` payload. `tick` must be monotonically non-decreasing across a
/// connection; that invariant is enforced by the orchestrator, not by this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub tick: u64,
}

/// OpenRCT2's in-game calendar has 8 months per year, not the Gregorian 12,
/// so `park_date` is this structured triple rather than an ISO calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkDate {
    pub year: u32,
    pub month: u8,
    pub day: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RideStatus {
    Open,
    Closed,
    Testing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ride {
    pub id: u32,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: RideStatus,
    pub price: u32,
    pub queue_length: u32,
    pub downtime: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaffCounts {
    pub handyman: u32,
    pub mechanic: u32,
    pub security: u32,
    pub entertainer: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weather {
    Sunny,
    PartiallyCloudy,
    Cloudy,
    Rain,
    HeavyRain,
    Thunder,
    Snow,
    HeavySnow,
}

/// `observation.snapshot` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationSnapshot {
    pub park_date: ParkDate,
    pub cash: i64,
    pub loan: u64,
    pub park_rating: u16,
    pub guest_count: u32,
    pub rides: Vec<Ride>,
    pub staff_counts: StaffCounts,
    pub weather: Weather,
}

/// `command.result` payload. The mandatory `correlation_id` referencing the
/// originating `command.request` lives on the envelope, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResult {
    pub engine_cost: Option<i64>,
    pub engine_error: Option<ErrorInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    OperatorRequest,
    FatalError,
    WatchdogTimeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shutdown {
    pub reason: ShutdownReason,
}

/// `ack` payload. Empty on purpose: an ack's meaning is entirely carried by the
/// envelope's mandatory `correlation_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ack {}
