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
