-- Phase 8 (AFK safety net): the explicit safety-state machine
-- (NORMAL/CAUTIOUS/CONSERVATION/QUARANTINE/ROLLBACK/STOPPED) docs/VISION.md
-- calls for and ADR-0003/ADR-0005 deferred here. The current state is
-- derived by reading the latest row, not maintained as separate mutable
-- state -- see docs/DECISIONS.md ADR-0006. Global, not per-simulation:
-- 0.1 has at most one active simulation at a time, and the watchdog can
-- transition state before any simulation has connected (simulation_id
-- NULL in that case).
CREATE TABLE state_transitions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id   UUID REFERENCES simulations (id),
    from_state      TEXT NOT NULL,
    to_state        TEXT NOT NULL,
    reason          TEXT NOT NULL,
    triggered_by    TEXT NOT NULL CHECK (triggered_by IN ('orchestrator', 'watchdog', 'manual')),
    -- Only set when to_state = 'conservation': the tick at which this
    -- transition self-clears back to normal. NULL for every other state
    -- (cautious clears on a heartbeat count, quarantine/stopped need a
    -- manual `orchestrator resolve`, rollback is momentary).
    expires_at_tick BIGINT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX state_transitions_created_at_idx ON state_transitions (created_at DESC);
CREATE TRIGGER state_transitions_append_only
    BEFORE UPDATE OR DELETE ON state_transitions
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();
