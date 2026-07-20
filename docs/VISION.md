# Vision

## Thesis

> Helter Skelter is an institutional intelligence that governs a historically
> coherent amusement park across decades, learns from the failures of the
> amusement industry, generates defensible expansions, and becomes the
> shared place of memory for a persistent artificial population.

Treat this as a serious long-term software and research program, not a
novelty chatbot integration. Helter Skelter must become:

1. An autonomous park founder and operator.
2. A historically constrained architectural intelligence.
3. A laboratory for industrial success and failure.
4. A generative roller-coaster design system.
5. A persistent artificial society embedded in OpenRCT2.
6. An institutional intelligence with a doctrine and personality that emerges
   from its own accumulated decisions, not from arbitrary prose.

## Local-first

The useful system must run entirely on the operator's local Linux
workstation (RTX 5080-class GPU today, scaling to RTX 5090 / RTX PRO-class
hardware later). Cloud deployment must be easy to add, but cloud services
must never be a hard dependency for core operation. On local hardware,
prioritize quantized models, bounded context, batching, caching, structured
outputs, and GPU scheduling over raw model size.

## AFK with deterministic guardrails

Once launched, Helter Skelter operates for long periods without a human
approving ordinary decisions. That autonomy is only safe because every
mutation is deterministic and accountable:

- **No blind autonomy.** AFK operation requires permissions, budgets,
  snapshots, validation, rollback, watchdogs, rate limits, confidence
  thresholds, and an immutable decision log — not vibes.
- **Proposal → authorization, always.** Nothing touches the live park state
  without passing through the governor: proposal → assumptions → prediction
  → authorization → execution → observed result → prediction error → causal
  analysis → lesson → confidence → policy update.
- **The system may conclude that no action is justified.** Inaction is a
  valid, and often correct, decision.
- **Failure is instrumented, not hidden.** Any proposed autonomous feature
  must explain how it fails safely — snapshot-and-rollback, dry-run/clone
  simulation, heartbeats, deadlock and oscillation detection, safe mode.

## Institutional intelligence, not a single model

OpenRCT2 is the simulation body: the plugin is a bridge, observer, command
translator, and telemetry layer only. Heavy reasoning — planning, historical
analysis, causal postmortems, generative design — lives outside the plugin,
in Rust and (from 0.2+) Python services. The park is governed as a body with
distinct responsibilities (founder, architect, engineer, historian,
treasurer, operator, safety officer, historian-of-failure, and the governor
who arbitrates), each producing structured proposals with assumptions,
confidence, cost, and risk — not free-form debate.

Default constitutional priority order, highest first:

1. Guest survival and simulation integrity.
2. Financial survival.
3. Operational resilience.
4. Historical coherence.
5. Visitor welfare and satisfaction.
6. Long-term artistic identity.
7. Expansion and prestige.

## Historical coherence is systemic

An era constrains technology, manufacturers, economics, design language,
operations, regulation, media, and social expectations — it is not a
cosmetic skin. Alternate-history divergence is allowed but must always be
recorded explicitly. Industrial failures (bankruptcies, accidents, botched
expansions) are represented as causal records with competing explanations
and preserved uncertainty, never reduced to a single simplistic cause.

## No fake consciousness claims

From 0.3 onward, visitors are simulated persons with persistent
autobiographical and social models — identity, preferences, fears, beliefs,
memories, relationships. The system never claims proof of phenomenal
consciousness. Cognition is hierarchical and budgeted: population-level
utility AI for the crowd, a group layer, a salient-person layer, and a
narrow, rotating, event-driven layer of bounded model calls for the guests
who matter right now. Nothing runs a full model call per visitor per tick.

## Evidence over theatricality

Every proposed feature must identify its data inputs, state model,
algorithms, failure modes, test strategy, and computational cost. Sessions
working on this repository inspect the actual files before asserting what
exists — see [CLAUDE.md](../CLAUDE.md).
