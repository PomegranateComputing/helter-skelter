-- Phase 7 (park snapshots and rollback): snapshots need a game-tick label
-- to answer "is there a snapshot newer than T ticks", and rollbacks need
-- their own append-only ledger table -- see docs/DECISIONS.md ADR-0005.

ALTER TABLE snapshots ADD COLUMN tick BIGINT NOT NULL;

-- A rollback event: always references the snapshot it restored (or would
-- restore) to. Append-only, like every other ledger table -- a rollback
-- is a historical fact, never edited after it happens.
CREATE TABLE rollbacks (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id UUID NOT NULL REFERENCES simulations (id),
    snapshot_id   UUID NOT NULL REFERENCES snapshots (id),
    reason        TEXT NOT NULL,
    triggered_by  TEXT NOT NULL CHECK (triggered_by IN ('manual', 'automatic')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX rollbacks_simulation_id_idx ON rollbacks (simulation_id, created_at);
CREATE TRIGGER rollbacks_append_only
    BEFORE UPDATE OR DELETE ON rollbacks
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();
