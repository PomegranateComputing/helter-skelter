# Architecture Decision Records

Format: one entry per decision, appended in order, never rewritten. Each
entry has a status, the context that forced the decision, the decision
itself, and its consequences.

```
## ADR-XXXX: <title>

- Status: proposed | accepted | superseded by ADR-YYYY
- Date: YYYY-MM-DD

### Context

Why this decision was necessary.

### Decision

What was decided.

### Consequences

What this makes easier, harder, or forecloses.
```

---

## ADR-0001: Adopt existing tree; restrict 0.1 scope; GOG assets untracked

- Status: accepted
- Date: 2026-07-20

### Context

The repository skeleton (directories and empty files) already existed
before this session, matching `HelterSkelter_Project_Overview.txt`. A local
GOG copy of RollerCoaster Tycoon 2 was already extracted under
`assets/gog/` (~1.7 GB), which is copyrighted game data, not project
source. Milestone 0.1 ("The Autonomous Operator") needs only a narrow
vertical slice of the full target architecture to reach its goal: observe
OpenRCT2, propose a bounded action, get it authorized, execute it, log the
result.

### Decision

1. Adopt the existing directory skeleton as-is. Do not restructure, rename,
   or delete any existing directory, even ones unused in 0.1.
2. Milestone 0.1 only writes code under: `bridge/openrct2-plugin`,
   `bridge/protocol`, `bridge/messages`, `core/orchestrator`,
   `core/world-model`, `core/common`, `core/governor`, `db/`, `config/`,
   `scripts/`, `tests/`, `docs/`, and `docker-compose.yml`. Every other
   directory (`core/coaster-foundry`, `core/visitor-engine`,
   `core/history-engine`, `core/economy`, `core/events`,
   `core/failure-analysis`, `core/simulation`, `services/*`, `datasets/`,
   `knowledge/`, `models/`, `papers/`, `research/`, `experiments/`) gets a
   `.gitkeep` and stays empty until its milestone arrives.
3. `assets/gog/` is excluded from version control entirely via
   `.gitignore`. `assets/gog/README.md` documents how to re-extract the
   installer locally, so the repository never has to carry copyrighted game
   data.

### Consequences

- Future sessions can trust the directory layout as ground truth and must
  not invent new top-level directories to "improve" the structure.
- A session that finds itself writing code outside the 0.1 scope list
  above is off-track and should stop and check `docs/ROADMAP.md`.
- Cloning this repository never gives you a playable game — GOG assets are
  restored locally per `assets/gog/README.md`, once, outside of git.

---

## ADR-0002: Bridge<->orchestrator protocol — no cross-file $ref; ack requires correlation_id; fixtures are the sync mechanism

- Status: accepted
- Date: 2026-07-20

### Context

The bridge (TypeScript, in-process with OpenRCT2) and the orchestrator
(Rust, external process) need a versioned wire protocol before either can
do anything real. The task specified an envelope shape, seven message
kinds, and five bounded actions, plus "a CI check that schemas and both
type sets stay in sync" — but not how.

Two design questions came up that the spec didn't settle:

1. Whether `ack`, like `command.result`, should be forced to carry a
   non-null `correlation_id`. Only `command.result` was named explicitly.
2. How to link a `kind`-specific (or `action`-specific) payload schema to
   the envelope schema. JSON Schema's `$ref` can do this across files, but
   both `ajv` and the Rust `jsonschema` crate resolve `$ref` by URI, which
   means either fake network-shaped `$id`s plus a custom in-memory
   resolver, or it silently doesn't work across files at all.

### Decision

1. `ack`'s `correlation_id` is mandatory (non-null), same as
   `command.result`, even though the task only named the latter. An ack
   with no reference to what it's acknowledging is structurally
   meaningless — this is a "no free-form text where structure fits"
   judgment call, recorded here so it isn't mistaken for scope creep.
2. Every file under `bridge/protocol/` is independently loadable and
   compilable — no cross-file `$ref`. `envelope.schema.json` validates
   `payload` only as `{"type": "object"}`; `command_request.schema.json`
   validates `params` the same way. The kind↔payload and action↔params
   correspondence is enforced instead by (a) adjacently-tagged Rust enums
   and discriminated TS unions, which make a mismatched pairing a compile
   error, and (b) the test harness, which looks up and validates against
   the right schema file given a fixture's `kind`/`action`.
3. `bridge/messages/fixtures/*.json` (12 fixtures: one per message kind,
   one `command.request` per bounded action, one `command.result` each
   for ok/error) is the single source of truth. `core/common/tests/protocol_roundtrip.rs`
   and `bridge/openrct2-plugin/test/protocol_roundtrip.test.js` both
   validate every fixture against the schemas and round-trip it through
   the language's own types. There is no separate "sync check" CI job —
   these two test suites, run by the existing `rust` and `bridge` CI jobs,
   are the sync check.
4. A planned compile-time TS check (statically import each fixture and
   assert `satisfies Envelope`) was tried and abandoned: TypeScript widens
   string-literal fields on JSON module imports (`kind: "hello"` becomes
   `kind: string`), so a discriminated union keyed on string literals can
   never structurally match an imported JSON value. The TS side relies on
   runtime `ajv` validation instead; see `docs/PROTOCOL.md`.

### Consequences

- Adding a message kind or bounded action touches four things every time
  (schema, fixture, Rust variant, TS variant) and is only proven complete
  when both test suites pass — an incomplete addition fails CI rather than
  silently drifting.
- The schemas are simpler to read (no `$ref`/`if`/`then` chains spanning
  files to trace) at the cost of the kind↔payload link being implicit
  (enforced by code and tests, not visible in the schema file itself).
  Future sessions should not "fix" this by adding `$ref` without also
  solving the resolver problem for both `ajv` and the `jsonschema` crate.
- `docs/PROTOCOL.md`'s versioning policy (new `protocol_version` for any
  breaking change, no cross-version negotiation in 0.1) applies to this
  entire message set from this commit forward.

---

## ADR-0003: Durable memory — PostgreSQL, append-only ledger via triggers, DB loss degrades rather than crashes

- Status: accepted
- Date: 2026-07-20

### Context

`core/orchestrator` needed durable storage for the decision ledger
(`docs/VISION.md`: proposal → assumptions → prediction → authorization →
execution → result) and for every `observation.snapshot`. Three decisions
weren't settled by the task description and needed a call: how "append-only"
is enforced, how `sqlx`'s compile-time query checking interacts with CI,
and what "DB loss degrades to CAUTIOUS" actually means before the fuller
AFK safety-state machine (`NORMAL`/`CAUTIOUS`/`CONSERVATION`/`QUARANTINE`/
`ROLLBACK`/`STOPPED`) exists — that's a later milestone, not this one.

### Decision

1. **PostgreSQL 16**, run via `docker-compose.yml`, one `db` service, named
   volume, healthcheck. Locally this repo's compose file maps it to host
   port **5433**, not the default 5432 — this development machine already
   has something else bound to 5432 unrelated to this project, and
   remapping our own service is far less risky than touching a port we
   don't own. CI's `postgres` service container uses the real 5432 inside
   its own fresh runner, no conflict there.
2. **Append-only is enforced by a database trigger**
   (`prevent_update_delete()`), not just by omitting `UPDATE`/`DELETE`
   code in the orchestrator. Applied to every ledger table
   (`observations`, `proposals`, `authorizations`, `actions`,
   `action_results`, `snapshots`); `simulations` is the one mutable
   table, since it needs `ended_at` set when a simulation ends. A ledger
   that's only immutable "by convention" isn't immutable — a future bug
   or an ad hoc `psql` session could corrupt history otherwise.
3. **`sqlx::query!`/`query_scalar!` compile-time query checking connects
   to a live database at build time** — there is no offline
   (`.sqlx` cache) mode in this setup. This means `DATABASE_URL` must
   point at an already-migrated database before `cargo build`,
   `cargo clippy`, or `cargo test` run, not just before the tests
   themselves execute. CI's `rust` job therefore starts a `postgres`
   service, runs `sqlx migrate run` (then `revert` then `run` again, to
   exercise reversibility per the task's own request), and only then
   runs `fmt`/`clippy`/`test`. Anyone building locally needs
   `make db-up && make db-migrate` first — this is a real, load-bearing
   ordering requirement, not a suggestion.
4. **DB loss degrades to a `db_state: cautious` flag on `/health`, buffers
   writes in a bounded channel, and retries with capped exponential
   backoff — it does not adopt the full `CAUTIOUS`/`CONSERVATION`/etc.
   safety-state machine.** That state machine (with its cross-cutting
   effects on the governor, action budgets, etc.) is explicitly a later
   milestone's task. Introducing it fully here, ahead of the systems it's
   meant to gate, would be scope creep disguised as thoroughness. What
   0.1 needs — writes survive a DB blip without data loss or a crash — is
   what's implemented: `core/orchestrator/src/db.rs`'s `Persistence`
   worker retries each job indefinitely (capped backoff) rather than
   giving up and dropping ledger data, and the bounded channel capacity
   (500) is the actual "buffer" limit; only a sustained outage past that
   capacity drops new submissions (logged).

### Consequences

- Adding a new persisted fact means adding both a migration (with a
  reversible `.down.sql`) and a `PersistJob` variant — the two are
  expected to be added together, same as the protocol's four-artifacts
  pattern in ADR-0002.
- Nobody can `cargo build` this workspace without a reachable, migrated
  Postgres. This is a meaningfully different developer experience from
  the rest of 0.1 (which needed no external services to compile) and
  should be called out prominently in onboarding docs/README, not left
  as a surprise the first time someone's build fails with a connection
  error.
- The real `CAUTIOUS`/etc. safety-state machine, when it's built, will
  need to reconcile with this ADR's narrower `db_state` flag rather than
  design against a blank slate — check `core/orchestrator/src/state.rs`'s
  `DbState` and `db.rs`'s retry loop first.

---

## ADR-0004: Operator slice — the first real decision pipeline (set_ride_price)

- Status: accepted
- Date: 2026-07-20

### Context

This milestone's task was to wire one full decision loop end to end —
snapshot → deterministic rule → proposal → authorization → query/execute
→ result → persisted outcome — for exactly one bounded command,
`set_ride_price`, with no LLM anywhere in it. The task description didn't
settle where the new proposal/authorization/governor types should live,
how "query then execute" maps onto the bridge's actual plugin API, how
in-memory governor state should behave across orchestrator restarts, or
how to keep `idempotency_key` unique under the schema's constraints —
each needed a call.

### Decision

1. **`core/governor` owns `Proposal`, `Authorization`, `Decision`, and
   `Constitution`, not `core/orchestrator`.** The rule engine
   (`core/orchestrator/src/operator.rs`) constructs `Proposal`s but the
   types themselves belong with the policy that judges them — a future
   second rule-emitting agent (0.2+) should depend on `governor`'s types
   the same way `operator.rs` does, not duplicate them.
2. **Query then execute is one synchronous round trip inside a single
   `command.request`/`command.result` exchange, not two separate wire
   messages.** `bridge/openrct2-plugin/src/commands.ts`'s
   `handleCommandRequest` calls `context.queryAction` and, only if it
   reports no error, `context.executeAction`, and returns one
   `command.result` either way. The orchestrator never sees a
   query-only round trip — it authorizes once, sends one
   `command.request`, and gets back one outcome. This keeps the
   protocol's correlation model (one request, one correlated result)
   intact rather than inventing a second request/response pair just for
   this milestone.
3. **The governor's rate-limit and cooldown state is in-memory only,
   reset on orchestrator restart — not persisted.** Every individual
   authorization decision *is* durably persisted (`authorizations` rows
   with `reason` and `policy_version`), so the ledger itself is never
   incomplete; what resets is only the governor's own bookkeeping (hourly/
   daily counters, per-ride cooldown timestamps) used to make the *next*
   decision. Rebuilding that bookkeeping from the ledger on startup is
   real work (replaying wall-clock-scoped budgets against
   simulation-tick-scoped cooldowns) that this milestone's scope — one
   command type, no restart-survival requirement in the task — doesn't
   justify yet. A restart mid-day currently resets the daily/hourly
   budget early; flagged here so it isn't mistaken for an oversight
   later.
4. **`idempotency_key` is scoped by `simulation_id`, not just
   `ride_id`+tick.** The schema enforces a single global `UNIQUE`
   constraint on `actions.idempotency_key` (no per-simulation scoping),
   but `current_tick` legitimately repeats across simulations (it
   defaults to 0 until a simulation's first heartbeat arrives). Keying
   only on `ride_id`+tick produced identical keys across independent
   simulation runs and collided against the constraint — a real bug this
   design hit during integration testing, not a hypothetical. The key is
   now `operator-sim{simulation_id}-ride{ride_id}-tick{current_tick}`.
5. **`insert_simulation_start` is synchronous, not routed through the
   buffered `Persistence` worker used for observations.** Every row in
   the decision pipeline foreign-keys to `simulations`, so if that row's
   insert is buffered and hasn't landed yet when the very next
   `observation.snapshot` tries to insert a `proposals` row against the
   same `simulation_id`, the proposal insert fails with a foreign-key
   violation depending on how the two async tasks happen to interleave —
   again a real bug hit during integration testing (see `db.rs`'s module
   doc comment). The general rule this establishes: a table other tables
   foreign-key to must be written synchronously if the writer expects to
   insert dependents against it in the same logical flow; buffered writes
   stay safe only for leaf data with no same-request dependents.

### Consequences

- Any 0.2+ rule-proposing agent should live alongside `operator.rs` in
  `core/orchestrator` and import its proposal/authorization types from
  `core/governor`, not redefine them.
- A future milestone that needs authorization decisions to survive a
  restart (or needs multiple orchestrator instances sharing one
  governor's rate limits) will need to change the governor's state from
  in-memory to persisted/shared — this ADR's point 3 is the explicit
  marker for where that work starts.
- The real end-to-end proof run for this milestone could only exercise
  the price-*decrease* branch of the operator rule, not the increase
  branch: the bridge's `queue_length` field is a hardcoded `0` placeholder
  (no OpenRCT2 API exposes real ride queue length yet — see
  `docs/OPENRCT2_INTEGRATION.md`'s GAPS section), so "queue too long" can
  never be observed for real, while "queue empty" is trivially always
  true. The dev park's rides also start at `price: 0`, which is already
  the configured floor, so a one-off local plugin was used to seed both
  rides to a non-zero price before the real run so the decrease had room
  to fire; that seeding plugin is not part of the committed bridge. The
  increase branch is proven only by `core/orchestrator/src/operator.rs`'s
  and `tests/integration/tests/operator_slice.rs`'s unit/integration
  tests, which construct synthetic high-queue snapshots directly — not by
  a live OpenRCT2 run. This gap closes only once a real queue-length
  source exists in the engine or bridge.
