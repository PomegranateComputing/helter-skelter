use serde_json::Value;

/// A structured proposal from an agent (e.g. "The Operator") -- see
/// docs/VISION.md's proposal -> authorization -> execution loop. Mirrors
/// the `proposals` table (db/migrations/) field for field.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub agent: String,
    pub assumptions: Value,
    pub predicted_effect: Value,
    pub confidence: f32,
    pub cost_envelope: Value,
    pub expiry_tick: u64,
    /// Which ride this proposal concerns and what price it proposes --
    /// needed by the governor's price-bounds and per-ride-cooldown checks.
    /// 0.1 only proposes `set_ride_price` changes, so this is the whole
    /// shape a proposal needs; a future action type would need this
    /// generalized.
    pub ride_id: u32,
    pub proposed_price: i64,
}
