-- Generated from db/migrations/ -- do not hand-edit. Regenerate with:
--   docker exec helter-skelter-db-1 pg_dump -U helterskelter -d helterskelter \
--     --schema-only --no-owner --no-privileges --exclude-table=_sqlx_migrations

CREATE FUNCTION public.prevent_update_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    RAISE EXCEPTION '% is append-only: % is not permitted', TG_TABLE_NAME, TG_OP;
END;
$$;

CREATE TABLE public.action_results (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    action_id uuid NOT NULL,
    engine_cost bigint,
    engine_error jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.actions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    authorization_id uuid NOT NULL,
    command jsonb NOT NULL,
    idempotency_key text NOT NULL,
    expiry_tick bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.authorizations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    proposal_id uuid NOT NULL,
    decision text NOT NULL,
    reason text NOT NULL,
    policy_version text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT authorizations_decision_check CHECK ((decision = ANY (ARRAY['authorized'::text, 'rejected'::text])))
);

CREATE TABLE public.observations (
    id bigint NOT NULL,
    simulation_id uuid NOT NULL,
    message_id uuid NOT NULL,
    recorded_at timestamp with time zone NOT NULL,
    payload jsonb NOT NULL,
    cash bigint NOT NULL,
    guest_count integer NOT NULL,
    park_rating integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE public.observations_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.observations_id_seq OWNED BY public.observations.id;

CREATE TABLE public.proposals (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    simulation_id uuid NOT NULL,
    agent text NOT NULL,
    assumptions jsonb NOT NULL,
    predicted_effect jsonb NOT NULL,
    confidence real NOT NULL,
    cost_envelope jsonb NOT NULL,
    expiry_tick bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.simulations (
    id uuid NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    ended_at timestamp with time zone,
    bridge_version text NOT NULL,
    openrct2_version text NOT NULL
);

CREATE TABLE public.snapshots (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    simulation_id uuid NOT NULL,
    kind text NOT NULL,
    storage_path text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

ALTER TABLE ONLY public.observations ALTER COLUMN id SET DEFAULT nextval('public.observations_id_seq'::regclass);

ALTER TABLE ONLY public.action_results
    ADD CONSTRAINT action_results_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.actions
    ADD CONSTRAINT actions_idempotency_key_key UNIQUE (idempotency_key);

ALTER TABLE ONLY public.actions
    ADD CONSTRAINT actions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.authorizations
    ADD CONSTRAINT authorizations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.observations
    ADD CONSTRAINT observations_message_id_key UNIQUE (message_id);

ALTER TABLE ONLY public.observations
    ADD CONSTRAINT observations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.proposals
    ADD CONSTRAINT proposals_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.simulations
    ADD CONSTRAINT simulations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.snapshots
    ADD CONSTRAINT snapshots_pkey PRIMARY KEY (id);

CREATE INDEX action_results_action_id_idx ON public.action_results USING btree (action_id);

CREATE INDEX authorizations_proposal_id_idx ON public.authorizations USING btree (proposal_id);

CREATE INDEX observations_simulation_id_idx ON public.observations USING btree (simulation_id, recorded_at);

CREATE INDEX proposals_simulation_id_idx ON public.proposals USING btree (simulation_id, created_at);

CREATE INDEX snapshots_simulation_id_idx ON public.snapshots USING btree (simulation_id, created_at);

CREATE TRIGGER action_results_append_only BEFORE DELETE OR UPDATE ON public.action_results FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

CREATE TRIGGER actions_append_only BEFORE DELETE OR UPDATE ON public.actions FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

CREATE TRIGGER authorizations_append_only BEFORE DELETE OR UPDATE ON public.authorizations FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

CREATE TRIGGER observations_append_only BEFORE DELETE OR UPDATE ON public.observations FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

CREATE TRIGGER proposals_append_only BEFORE DELETE OR UPDATE ON public.proposals FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

CREATE TRIGGER snapshots_append_only BEFORE DELETE OR UPDATE ON public.snapshots FOR EACH ROW EXECUTE FUNCTION public.prevent_update_delete();

ALTER TABLE ONLY public.action_results
    ADD CONSTRAINT action_results_action_id_fkey FOREIGN KEY (action_id) REFERENCES public.actions(id);

ALTER TABLE ONLY public.actions
    ADD CONSTRAINT actions_authorization_id_fkey FOREIGN KEY (authorization_id) REFERENCES public.authorizations(id);

ALTER TABLE ONLY public.authorizations
    ADD CONSTRAINT authorizations_proposal_id_fkey FOREIGN KEY (proposal_id) REFERENCES public.proposals(id);

ALTER TABLE ONLY public.observations
    ADD CONSTRAINT observations_simulation_id_fkey FOREIGN KEY (simulation_id) REFERENCES public.simulations(id);

ALTER TABLE ONLY public.proposals
    ADD CONSTRAINT proposals_simulation_id_fkey FOREIGN KEY (simulation_id) REFERENCES public.simulations(id);

ALTER TABLE ONLY public.snapshots
    ADD CONSTRAINT snapshots_simulation_id_fkey FOREIGN KEY (simulation_id) REFERENCES public.simulations(id);

