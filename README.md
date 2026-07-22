# Helter Skelter

Helter Skelter is a local-first, cloudable, autonomous intelligence layer for
OpenRCT2. It runs AFK on the operator's own workstation, under deterministic
guardrails, to found, construct, govern, and remember an historically
coherent amusement park across years of simulated time — and eventually to
give every visitor a persistent identity within it. It does not merely play
RollerCoaster Tycoon; it is an institutional intelligence, accountable to a
written constitution of priorities, that learns from the amusement
industry's real successes and failures and keeps an immutable ledger of
every decision it makes.

See [docs/VISION.md](docs/VISION.md) for the full thesis and
[docs/ROADMAP.md](docs/ROADMAP.md) for the path from 0.1 to 1.0.

**Status: milestone 0.1 ("The Autonomous Operator") is complete**, tagged
[`v0.1.0`](../../releases/tag/v0.1.0). See docs/DECISIONS.md's ADR-0007 for
the acceptance-run retrospective and 0.2 ("The Architect") is next.

## Architecture (0.1 scope)

Milestone 0.1 — "The Autonomous Operator" — touches only the slice needed to
observe OpenRCT2, propose a bounded action, get it authorized, execute it,
and log the outcome:

```text
helter-skelter/
├── bridge/
│   ├── openrct2-plugin/   TypeScript OpenRCT2 plugin: observes park state,
│   │                      executes authorized game actions. No policy logic.
│   ├── protocol/          Versioned message schema shared by bridge + core.
│   └── messages/          Serialized message definitions/fixtures.
├── core/
│   ├── common/            Shared Rust types and utilities.
│   ├── world-model/        Rust representation of observed park state.
│   ├── governor/           Rust policy engine: proposal -> authorization.
│   └── orchestrator/       Rust process: bridge <-> governor <-> db loop.
├── db/                     PostgreSQL schema, migrations, seeds, snapshots.
├── config/                 Runtime configuration.
├── scripts/                Bootstrap, dev, import, and tooling scripts.
├── tests/                  Integration, simulation, and unit tests.
└── docs/                   Vision, roadmap, standards, ADRs.
```

Every other top-level directory (`core/coaster-foundry`, `core/visitor-engine`,
`core/history-engine`, `core/economy`, `core/events`, `core/failure-analysis`,
`core/simulation`, `services/*`, `datasets/`, `knowledge/`, `models/`,
`papers/`, `research/`, `experiments/`) is a placeholder reserved for
milestones 0.2 and later — see the scope rule in
[docs/ROADMAP.md](docs/ROADMAP.md).

## 0.1 goals (all met — see ADR-0007)

- Launch and observe OpenRCT2 in an automation-friendly mode.
- Export a complete park-state representation over the bridge.
- Monitor finance, rides, queues, staff, failures, and satisfaction.
- Propose bounded operational actions; require governor authorization before
  any are executed.
- Maintain an immutable decision ledger (proposal → assumptions → prediction
  → authorization → execution → result) in PostgreSQL.
- Run for hours unattended, and recover cleanly from a bridge or
  orchestrator crash.
- An explicit safety-state machine (Normal/Cautious/Conservation/
  Quarantine/Rollback/Stopped) with a separate watchdog process, snapshot
  + rollback, and crash recovery.

No language model sits in the 0.1 decision loop. Every action taken in this
milestone is deterministic and policy-driven — see
[CLAUDE.md](CLAUDE.md).

## Running it

Prerequisites: Rust/Cargo, Node.js + pnpm, Docker (for PostgreSQL), and a
local GOG copy of RollerCoaster Tycoon 2 extracted under `assets/gog/` (see
[assets/gog/README.md](assets/gog/README.md) — that data is copyrighted and
is never committed to this repository).

```bash
# Bring up PostgreSQL and apply migrations FIRST -- core/orchestrator's
# sqlx::query! macros connect to DATABASE_URL at compile time to check
# queries against the real schema, so this has to happen before any
# `cargo build`/`clippy`/`test`, not just before running the orchestrator.
# See docs/DECISIONS.md ADR-0003.
make db-up
make db-migrate

# Build the Rust workspace
cargo build --workspace

# Build the OpenRCT2 bridge plugin
cd bridge/openrct2-plugin && pnpm install && pnpm build

# Run the orchestrator (starts the observe -> propose -> authorize -> act loop)
cd - && DATABASE_URL=postgres://helterskelter:helterskelter@localhost:5433/helterskelter cargo run -p orchestrator
```

Or, to bring up the whole real stack (orchestrator + headless OpenRCT2 +
bridge plugin) in one step: `scripts/dev/run-stack.sh`.

### Operator CLI

`orchestrator` (no subcommand) runs the observe → propose → authorize →
act loop. Subcommands for operator intervention:

```bash
# Roll back to a snapshot (restores runtime/current-park.park; restart
# the stack afterward to load it):
orchestrator rollback --to <snapshot_id> [--reason "..."]

# Manually clear Quarantine/Stopped back to Normal (the only way out of
# either -- see docs/DECISIONS.md ADR-0006):
orchestrator resolve [--reason "..."]

# Generate a Markdown operator report for one simulation into exports/:
orchestrator report --simulation <simulation_id>
```

A separate watchdog binary (`cargo run -p orchestrator --bin watchdog`)
monitors the orchestrator's `/health` endpoint, the database, action rate,
and per-ride price oscillation from outside the orchestrator process —
see ADR-0006.

### Chaos and acceptance testing

`scripts/dev/chaos/` holds repeatable chaos tests (kill the bridge
mid-run, kill the orchestrator with an action in flight, a 60s database
outage). `scripts/dev/acceptance-0.1.sh [duration-seconds]` runs the full
0.1 acceptance criteria against the real stack — see ADR-0007.
