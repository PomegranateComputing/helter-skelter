/// The AFK safety-state machine docs/VISION.md's "no blind autonomy"
/// section calls for and docs/DECISIONS.md ADR-0003/ADR-0005 both
/// deferred to "a later milestone" -- this one. Every variant here is a
/// state the *whole system* (not any one ride or simulation) can be in;
/// only `Normal` authorizes proposals -- see
/// `docs/DECISIONS.md` ADR-0006 for why every other state, including the
/// comparatively mild `Cautious`, blocks authorization rather than
/// allowing a reduced/tentative form of it: a state machine with partial-
/// authorization states is a state machine with more edge cases to get
/// wrong, and 0.1's actions are cheap enough (a bounded price change) that
/// erring toward "don't act while uncertain" costs little.
///
/// The current state is derived by reading the latest row in the
/// `state_transitions` ledger (core/orchestrator/src/db.rs's
/// `current_safety_state`), not held as mutable process state -- Postgres
/// is the source of truth, exactly like every other decision this
/// project records, which is what makes crash recovery
/// (docs/DECISIONS.md ADR-0006) a reconciliation read instead of a
/// reconstruction problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyState {
    /// Steady state. The only state that authorizes proposals.
    Normal,
    /// Freshly (re)connected or otherwise unconfirmed -- e.g. right after
    /// an orchestrator restart, until two clean heartbeat cycles have
    /// been observed on the new connection. Self-clears.
    Cautious,
    /// Entered automatically after a rollback, for a bounded number of
    /// ticks (see `docs/DECISIONS.md` ADR-0005/ADR-0006). Self-clears.
    Conservation,
    /// Entered when the watchdog detects a misbehavior pattern (e.g.
    /// oscillation -- a ride's price reversing direction too many times
    /// in a window). Requires a manual `orchestrator resolve`.
    Quarantine,
    /// Momentary: entered immediately before a rollback's file restore
    /// and ledger recording, exited into `Conservation` right after. Its
    /// own ledger row exists so "a rollback happened" is queryable as a
    /// state, not just inferable from the `rollbacks` table.
    Rollback,
    /// The most severe state -- e.g. the database has been unreachable
    /// past its own threshold, or the action rate is wildly past normal.
    /// Requires a manual `orchestrator resolve`.
    Stopped,
}

impl SafetyState {
    /// Only `Normal` authorizes proposals -- see this type's doc comment.
    pub fn authorizes_proposals(self) -> bool {
        matches!(self, SafetyState::Normal)
    }

    /// `Quarantine` and `Stopped` don't self-clear on any timer or
    /// heartbeat count -- only a human running `orchestrator resolve`
    /// moves the system out of them.
    pub fn requires_manual_resolution(self) -> bool {
        matches!(self, SafetyState::Quarantine | SafetyState::Stopped)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SafetyState::Normal => "normal",
            SafetyState::Cautious => "cautious",
            SafetyState::Conservation => "conservation",
            SafetyState::Quarantine => "quarantine",
            SafetyState::Rollback => "rollback",
            SafetyState::Stopped => "stopped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "normal" => Some(SafetyState::Normal),
            "cautious" => Some(SafetyState::Cautious),
            "conservation" => Some(SafetyState::Conservation),
            "quarantine" => Some(SafetyState::Quarantine),
            "rollback" => Some(SafetyState::Rollback),
            "stopped" => Some(SafetyState::Stopped),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_normal_authorizes_proposals() {
        assert!(SafetyState::Normal.authorizes_proposals());
        for state in [
            SafetyState::Cautious,
            SafetyState::Conservation,
            SafetyState::Quarantine,
            SafetyState::Rollback,
            SafetyState::Stopped,
        ] {
            assert!(
                !state.authorizes_proposals(),
                "{state:?} must not authorize"
            );
        }
    }

    #[test]
    fn only_quarantine_and_stopped_require_manual_resolution() {
        assert!(SafetyState::Quarantine.requires_manual_resolution());
        assert!(SafetyState::Stopped.requires_manual_resolution());
        for state in [
            SafetyState::Normal,
            SafetyState::Cautious,
            SafetyState::Conservation,
            SafetyState::Rollback,
        ] {
            assert!(!state.requires_manual_resolution());
        }
    }

    #[test]
    fn as_str_and_parse_round_trip() {
        for state in [
            SafetyState::Normal,
            SafetyState::Cautious,
            SafetyState::Conservation,
            SafetyState::Quarantine,
            SafetyState::Rollback,
            SafetyState::Stopped,
        ] {
            assert_eq!(SafetyState::parse(state.as_str()), Some(state));
        }
    }

    #[test]
    fn parse_rejects_unknown_strings() {
        assert_eq!(SafetyState::parse("bogus"), None);
    }
}
