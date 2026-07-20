# Roadmap

## 0.1 — The Autonomous Operator (current)

- Launch and observe OpenRCT2 in an automation-friendly mode.
- Export a complete park-state representation over the bridge.
- Monitor finance, rides, queues, staff, failures, and satisfaction.
- Perform bounded operational actions, gated by the governor.
- Maintain an immutable decision ledger in PostgreSQL.
- Run for hours without intervention; recover cleanly from a bridge or
  orchestrator crash.

No language model sits in the 0.1 decision loop — every action is
deterministic and policy-driven. See [VISION.md](VISION.md) for the
constitutional priorities that gate every proposal.

### Scope rule for 0.1

0.1 only touches:

```
bridge/openrct2-plugin, bridge/protocol, bridge/messages,
core/orchestrator, core/world-model, core/common, core/governor,
db/, config/, scripts/, tests/, docs/, docker-compose.yml
```

Every other top-level directory is a placeholder for 0.2 and later —
`core/coaster-foundry`, `core/visitor-engine`, `core/history-engine`,
`core/economy`, `core/events`, `core/failure-analysis`, `core/simulation`,
`services/*`, `datasets/`, `knowledge/`, `models/`, `papers/`, `research/`,
`experiments/`. These directories hold a `.gitkeep` and no code until their
milestone arrives. See [CLAUDE.md](../CLAUDE.md) for the enforcement rule.

## 0.2 — The Architect

- Terrain graph and buildable-space representation.
- Zoning: entrance axis, anchors, lands, quiet zones, service corridors,
  reserved expansion.
- Path construction and candidate master plans.
- Placement of simple attractions and services.
- Clone-based evaluation and rollback before committing a plan.

Activates `core/world-model` extensions for terrain/zoning and, likely,
`services/api` for a dashboard to observe plans.

## 0.3 — The Population

- Persistent visitor identities: demographics, personality, preferences,
  fears, group membership, beliefs, episodic/semantic memory.
- Social groups, reputation, and rumor propagation.
- Return visits and aging across simulated years.

Activates `core/visitor-engine`, `knowledge/` (memory retrieval), and the
embedding/inference services under `services/`.

## 0.4 — The Foundry

- Constraint-driven, historically-styled roller-coaster generation
  (constraint solving, heuristic search, MCTS, genetic mutation of
  validated layouts — not an LLM placing track pieces directly).
- Validation for continuity, clearance, station connection, completion,
  speed/force proxies, capacity, cost, intensity, reliability, and
  historical plausibility.

Activates `core/coaster-foundry`.

## 1.0 — The Living Park

- Targeted C++ engine fork where the plugin API is genuinely insufficient.
- Decades of simulation: competing parks, acquisitions, bankruptcy, and
  institutional culture.
- Generational visitor populations.
- A stable, emergent, inspectable park personality (prestige, aggression,
  nostalgia, commercialism, experimentation, hospitality, risk aversion)
  that arises from recorded decisions and outcomes, not scripted prose.

Activates `core/history-engine`, `core/failure-analysis`, `core/economy`,
`core/events`, `core/simulation`, `datasets/`, `research/`, `papers/`.
