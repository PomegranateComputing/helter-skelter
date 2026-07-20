-- Milestone 0.1 schema: the decision ledger (proposal -> authorization ->
-- action -> result), the observation record, and snapshot bookkeeping.
-- See docs/DECISIONS.md ADR-0003 and docs/CODING_STANDARD.md.

-- Generic guard reused by every append-only table below: the ledger is
-- immutable by design (docs/VISION.md's "decision ledger"), so mutation
-- is rejected at the database level, not just by convention in
-- application code.
CREATE FUNCTION prevent_update_delete() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION '% is append-only: % is not permitted', TG_TABLE_NAME, TG_OP;
END;
$$ LANGUAGE plpgsql;

-- One row per running simulation (envelope.simulation_id). The only
-- table in this schema that isn't append-only: ended_at is set when the
-- simulation ends.
CREATE TABLE simulations (
    id                UUID PRIMARY KEY,
    started_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at          TIMESTAMPTZ,
    bridge_version    TEXT NOT NULL,
    openrct2_version  TEXT NOT NULL
);

-- One row per observation.snapshot received. Append-only: an observation
-- is a historical fact, never edited after the fact.
CREATE TABLE observations (
    id            BIGSERIAL PRIMARY KEY,
    simulation_id UUID NOT NULL REFERENCES simulations (id),
    message_id    UUID NOT NULL UNIQUE,
    recorded_at   TIMESTAMPTZ NOT NULL,
    payload       JSONB NOT NULL,
    -- Typed columns for the metrics queried often enough to want an index
    -- rather than a JSONB path expression every time; `payload` remains
    -- the source of truth for everything else in the snapshot.
    cash          BIGINT NOT NULL,
    guest_count   INTEGER NOT NULL,
    park_rating   INTEGER NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX observations_simulation_id_idx ON observations (simulation_id, recorded_at);
CREATE TRIGGER observations_append_only
    BEFORE UPDATE OR DELETE ON observations
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();

-- A structured proposal from an agent (e.g. "The Operator") -- see
-- docs/VISION.md's proposal -> authorization -> execution loop.
CREATE TABLE proposals (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id   UUID NOT NULL REFERENCES simulations (id),
    agent           TEXT NOT NULL,
    assumptions     JSONB NOT NULL,
    predicted_effect JSONB NOT NULL,
    confidence      REAL NOT NULL,
    cost_envelope   JSONB NOT NULL,
    expiry_tick     BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX proposals_simulation_id_idx ON proposals (simulation_id, created_at);
CREATE TRIGGER proposals_append_only
    BEFORE UPDATE OR DELETE ON proposals
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();

-- The governor's answer to a proposal -- authorized or rejected, always
-- with a reason and the policy version that produced the decision.
CREATE TABLE authorizations (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    proposal_id    UUID NOT NULL REFERENCES proposals (id),
    decision       TEXT NOT NULL CHECK (decision IN ('authorized', 'rejected')),
    reason         TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX authorizations_proposal_id_idx ON authorizations (proposal_id);
CREATE TRIGGER authorizations_append_only
    BEFORE UPDATE OR DELETE ON authorizations
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();

-- A command sent to the bridge after authorization. idempotency_key is
-- UNIQUE so replaying it is a conflict the caller must handle (see
-- docs/PROTOCOL.md: the orchestrator must treat repeated delivery of the
-- same key as a no-op after the first execution).
CREATE TABLE actions (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    authorization_id  UUID NOT NULL REFERENCES authorizations (id),
    command           JSONB NOT NULL,
    idempotency_key   TEXT NOT NULL UNIQUE,
    expiry_tick       BIGINT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER actions_append_only
    BEFORE UPDATE OR DELETE ON actions
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();

-- The command.result for an action -- engine_cost/engine_error mirror
-- bridge/protocol/messages/command_result.schema.json.
CREATE TABLE action_results (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action_id    UUID NOT NULL REFERENCES actions (id),
    engine_cost  BIGINT,
    engine_error JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX action_results_action_id_idx ON action_results (action_id);
CREATE TRIGGER action_results_append_only
    BEFORE UPDATE OR DELETE ON action_results
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();

-- A park-state snapshot's on-disk location. `kind` distinguishes how it
-- was produced (e.g. 'autosave', 'manual') -- see
-- docs/OPENRCT2_INTEGRATION.md's save-triggering gap for why this
-- milestone can't yet produce snapshots on demand.
CREATE TABLE snapshots (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    simulation_id UUID NOT NULL REFERENCES simulations (id),
    kind          TEXT NOT NULL,
    storage_path  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX snapshots_simulation_id_idx ON snapshots (simulation_id, created_at);
CREATE TRIGGER snapshots_append_only
    BEFORE UPDATE OR DELETE ON snapshots
    FOR EACH ROW EXECUTE FUNCTION prevent_update_delete();
