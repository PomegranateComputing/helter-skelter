# Coding Standard

## Rust

- **No `unwrap()` or `expect()` in production code paths.** They are
  permitted only in `#[cfg(test)]` code and `#[test]` functions. Every
  fallible operation on a path that can run during an unattended AFK
  session returns a typed `Result`.
- **Typed errors, not strings.** Each crate defines its own error enum
  (e.g. via `thiserror`) rather than passing around `String` or
  `Box<dyn Error>` across module boundaries. Callers must be able to match
  on failure modes, because failure mode determines whether the governor
  retries, rolls back, or halts.
- **Clippy and rustfmt are enforced, not advisory.** CI runs
  `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D
  warnings`. A PR that fails either does not merge.
- Prefer deterministic algorithms over model calls for deterministic
  problems (pathfinding, finance, geometry, scheduling, constraint
  checking). Reach for a model only where interpretation, abstraction, or
  genuinely uncertain causal reasoning adds value.

## Database

- All schema changes are `sqlx` migrations under `db/migrations/`, checked
  into version control, forward-only. No hand-applied schema changes
  against a running database.
- Migrations are reviewed for reversibility and for their effect on
  in-flight decision-ledger rows — the ledger is append-only and must never
  be rewritten by a migration.

## Commits

- [Conventional Commits](https://www.conventionalcommits.org/) —
  `feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:` — with a
  scope where useful, e.g. `feat(governor): add spending-budget check`.

## Tests

- **Every policy rule the governor enforces has a test.** If the governor
  can reject or gate a proposal for a reason, there is a test that
  constructs that exact scenario and asserts the rejection/gate — a rule
  without a test is not considered implemented.
- Integration tests exercising the bridge↔orchestrator↔db loop live under
  `tests/integration/`; simulation-level tests under `tests/simulation/`;
  unit tests live next to the code they test (`core/*/src/`) or under
  `tests/unit/` where a unit spans a crate boundary.
- A bug fix includes a regression test reproducing the original failure
  before the fix, in the same commit as the fix.
