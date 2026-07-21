/// A lightweight, 0.1-scoped safety mode -- not the full
/// NORMAL/CAUTIOUS/CONSERVATION/QUARANTINE/ROLLBACK/STOPPED state machine
/// docs/DECISIONS.md ADR-0003 defers to a later milestone. Phase 7 only
/// needs the one transition it was asked for: an automatic rollback
/// trigger lands the governor in `Conservation` for a bounded number of
/// ticks, during which no proposal is authorized, then it reverts to
/// `Normal` on its own. See ADR-0005.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Conservation {
        until_tick: u64,
    },
}
