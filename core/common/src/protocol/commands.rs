use serde::{Deserialize, Serialize};

/// The five bounded actions the governor may authorize in milestone 0.1 --
/// see `docs/PROTOCOL.md` and `bridge/protocol/commands/*.schema.json`.
/// No other action exists in this milestone; adding one requires a schema,
/// a fixture, and a matching TS variant, not just a new enum arm here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "params", rename_all = "snake_case")]
pub enum CommandAction {
    SetRidePrice { ride_id: u32, price: u32 },
    SetParkEntryFee { price: u32 },
    HireStaff { r#type: StaffType },
    OpenRide { ride_id: u32 },
    CloseRide { ride_id: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaffType {
    Handyman,
    Mechanic,
    Security,
    Entertainer,
}

/// `command.request` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    #[serde(flatten)]
    pub action: CommandAction,
    pub idempotency_key: String,
    pub expiry_tick: u64,
}
