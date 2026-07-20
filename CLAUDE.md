# CLAUDE.md

Condensed operating rules for any Claude session working in this repository.
Full context: [docs/VISION.md](docs/VISION.md), [docs/ROADMAP.md](docs/ROADMAP.md),
[docs/DECISIONS.md](docs/DECISIONS.md).

## Scope rule (0.1)

Milestone 0.1 only writes code under: `bridge/openrct2-plugin`,
`bridge/protocol`, `bridge/messages`, `core/orchestrator`,
`core/world-model`, `core/common`, `core/governor`, `db/`, `config/`,
`scripts/`, `tests/`, `docs/`, `docker-compose.yml`.

Every other top-level directory (`core/coaster-foundry`,
`core/visitor-engine`, `core/history-engine`, `core/economy`, `core/events`,
`core/failure-analysis`, `core/simulation`, `services/*`, `datasets/`,
`knowledge/`, `models/`, `papers/`, `research/`, `experiments/`) is a
0.2+ placeholder. Keep the `.gitkeep`, write nothing else there until its
milestone arrives per `docs/ROADMAP.md`. If a task seems to require code
there, stop and say so instead of proceeding.

## Never invent repository facts

Inspect the actual files before asserting what exists — read the directory,
`grep`, or open the file. Do not assume a module, function, or doc section
exists because it would be reasonable for it to exist. This applies to
recommending code, citing docs, and reporting task completion.

## Evidence over claims

Every proposed feature must identify its data inputs, state model,
algorithm, failure modes, test strategy, and computational cost before
being built. Do not report success without having run the build/test/lint
that would reveal failure.

## No LLM in the 0.1 decision loop

0.1's observe → propose → authorize → execute → log loop is entirely
deterministic. Do not introduce a language-model call into any path that
mutates park state in this milestone — that arrives, gated, starting in
0.3 (visitor cognition) and 0.4 (coaster foundry), and even then only for
interpretation/design-intent, never for direct execution of a mutation.

## All mutations go through proposal → authorization

No code path touches live OpenRCT2 state directly. The sequence is always:
proposal → assumptions → prediction → governor authorization → execution →
observed result → logged to the decision ledger. A change that skips the
governor is a bug regardless of how safe it looks in isolation.

## GOG assets are never committed

`assets/gog/` is gitignored on purpose — it holds a locally-extracted GOG
copy of RollerCoaster Tycoon 2 (copyrighted game data, ~1.7 GB). Never
suggest committing anything under it, and never remove it from
`.gitignore`. See `assets/gog/README.md` for how it's populated.

## Rust standard

No `unwrap()`/`expect()` outside tests, typed errors per crate, `cargo fmt
--check` and `cargo clippy -- -D warnings` clean, `sqlx` migrations only.
Every governor policy rule ships with a test that exercises it. Full detail
in `docs/CODING_STANDARD.md`.
