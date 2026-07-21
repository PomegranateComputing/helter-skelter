#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Authorized,
    Rejected,
}

/// The governor's answer to a proposal. Always has a reason, whether
/// authorized or rejected -- "no action justified" is a first-class,
/// fully-explained outcome, not a silent no-op. Mirrors the
/// `authorizations` table.
#[derive(Debug, Clone)]
pub struct Authorization {
    pub decision: Decision,
    pub reason: String,
    pub policy_version: String,
}
