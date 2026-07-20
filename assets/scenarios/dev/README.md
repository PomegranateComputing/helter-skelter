# assets/scenarios/dev/

`dev-park.park` is the fixture used by `scripts/dev/run-headless.sh` and the
bridge/orchestrator dev loop. It is **not** derived from the GOG RCT2
install — see provenance below.

## Provenance

- **Source:** `test/tests/testdata/parks/testReversedTrains.park` from the
  [OpenRCT2](https://github.com/OpenRCT2/OpenRCT2) project itself, tag
  `v0.5.3` (commit `f503f57bdb74b31507f83909db587a5db5794ef0`).
- **License:** GPL-3.0, same as the OpenRCT2 project as a whole (there is
  no separate license notice under `test/tests/testdata/`). It's a test
  fixture the OpenRCT2 project ships and uses in its own CI, not a
  recreation of a real-world park, so there's no third-party park-designer
  copyright question the way there would be for e.g. `bpb.sv6` (a
  Blackpool Pleasure Beach recreation) in the same test-data directory —
  that file was deliberately **not** used here for that reason.
- **Chosen because:** it's small (43 KB) and, confirmed by rendering it
  with `openrct2-cli screenshot ... giant`, contains exactly two compact
  coaster layouts — a good match for "2-3 rides" without pulling in a
  sprawling park.

## Why this is safe to commit

Per `.gitignore` and `assets/gog/README.md`, copyrighted GOG-extracted game
data (`assets/gog/`) is never committed. This file is unrelated to that
data: it was authored by the OpenRCT2 project, is GPL-3.0 licensed, and
contains no extracted GOG assets itself (a `.park` save references ride
*object identifiers*, like any scenario file — the proprietary graphics
those identifiers resolve to still come from the player's own local GOG
install at `assets/gog/`, exactly as for any other scenario).

## Regenerating

```bash
curl -fsSL https://raw.githubusercontent.com/OpenRCT2/OpenRCT2/v0.5.3/test/tests/testdata/parks/testReversedTrains.park \
  -o assets/scenarios/dev/dev-park.park
```
