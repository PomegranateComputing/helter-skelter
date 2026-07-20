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
