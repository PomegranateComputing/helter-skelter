# OpenRCT2 Integration

Verified facts only. Every claim below cites a file (and, where useful, a
line range) in [OpenRCT2/OpenRCT2](https://github.com/OpenRCT2/OpenRCT2)
at tag **`v0.5.3`** (commit `f503f57bdb74b31507f83909db587a5db5794ef0`), the
version this project is pinned to (`scripts/bootstrap/setup-openrct2.sh`).
Facts about the shipped TypeScript plugin API additionally cite
`doc/openrct2.d.ts` as packaged in the `v0.5.3` release tarball itself.
Where something is inferred or not yet confirmed, it's called out as such
rather than stated as fact — see "Gaps" at the end.

## Release, install, headless mode

- Official Linux Ubuntu-24.04(noble)/25.04(resolute)/Debian(bookworm/trixie)
  binary tarballs and an AppImage are published per release; our workstation
  (Ubuntu 26.04, codename `resolute`) has an exact-match tarball,
  `OpenRCT2-v0.5.3-Linux-resolute-x86_64.tar.gz`, verified against the
  release's `sha256sums.txt`.
- `openrct2-cli` is a distinct binary from `openrct2` (the windowed client).
  Its `main()` unconditionally sets `gOpenRCT2Headless = true;
  gOpenRCT2NoGraphics = true;` before running — `src/openrct2-cli/Cli.cpp`.
- The windowed `openrct2` binary can also run headless via the `--headless`
  flag, which is parsed and applied in `HandleCommandDefault()` —
  `src/openrct2/command_line/RootCommands.cpp:59,77,202-204`.
- Relevant CLI flags, all defined in `kStandardOptions` —
  `src/openrct2/command_line/RootCommands.cpp:69-92`:
  - `--headless` — run headless.
  - `--verbose` — enable `LOG_VERBOSE` output (needed for our asset-load
    proof below).
  - `--user-data-path=<path>` — override the directory containing
    `config.ini`.
  - `--openrct2-data-path=<path>` — override the OpenRCT2 asset data dir
    (languages, shaders, etc. — bundled in the release tarball's `data/`).
  - `--rct2-data-path=<path>` — override the RCT2 game data dir directly on
    the command line, bypassing `config.ini` entirely (containing
    `Data/g1.dat`).
  - `--port` / `--address` — multiplayer host/join port and bind address.
- In headless mode, the interactive RCT2-path-prompt fallback is skipped
  entirely: `if (!gOpenRCT2Headless) { rct2InstallPath =
  GetOrPromptRCT2Path(); ... }` — `src/openrct2/Context.cpp:399-407`. This
  means **headless mode has no fallback if `game_path` isn't already
  configured** — our setup script must configure it before any headless
  run, not rely on an interactive prompt.

## Configuring the RCT2 data path

- `config.ini`'s `[general]` section key is **`game_path`** (not
  `rct2_path`) — read at `src/openrct2/config/Config.cpp:200`, written at
  `:311`. Section name confirmed at `Config.cpp:181` (read) / `:296`
  (write).
- Default location on Linux: `$XDG_CONFIG_HOME/OpenRCT2/config.ini`, or
  `~/.config/OpenRCT2/config.ini` if `XDG_CONFIG_HOME` is unset —
  `src/openrct2/platform/Platform.Linux.cpp:84-99` (resolves
  `SpecialFolder::userConfig`) combined with
  `src/openrct2/PlatformEnvironment.cpp:262` (appends the `"OpenRCT2"`
  subdirectory) and `:66` (maps `PathId::config` to the filename
  `config.ini`). Note: on Linux, `userCache`/`userConfig`/`userData` all
  resolve to the *same* `$XDG_CONFIG_HOME`-based path
  (`Platform.Linux.cpp:84-99`) — OpenRCT2 does not split them across
  `~/.cache`/`~/.config`/`~/.local/share` the way the XDG spec suggests for
  those categories.
- **Official, validated way to set it:** `openrct2-cli set-rct2 <path>` —
  checks the path exists, checks `<path>/Data/g1.dat` exists, then writes
  `game_path` and saves `config.ini` —
  `src/openrct2/command_line/RootCommands.cpp:337-390`. This is what
  `scripts/bootstrap/setup-openrct2.sh` uses, rather than hand-editing the
  INI (safer: it preserves every other existing config value, since it
  loads the existing file first via `Config::OpenFromPath` before
  overwriting only `rct2Path`).
- **Caveat found empirically:** `set-rct2` does not call
  `HandleCommandDefault()`, so a `--verbose` flag passed alongside it is
  parsed but never applied to `_log_levels` — it prints no proof of
  anything. Use `scan-objects --verbose` afterward for that (below).
- **Proof-of-load mechanism:** `CreatePlatformEnvironment()` always loads
  `config.ini` and sets the RCT2 base path from `Config::Get().general.rct2Path`
  (unless overridden by `--rct2-data-path`), then logs it —
  `src/openrct2/PlatformEnvironment.cpp:293-311`:
  ```cpp
  LOG_VERBOSE("DirBase::rct2    : %s", env->GetDirectoryPath(DirBase::rct2).c_str());
  ```
  Running `openrct2-cli scan-objects --verbose` (which calls
  `HandleCommandDefault()` first, so `--verbose` takes effect —
  `src/openrct2/command_line/RootCommands.cpp:392-408`) prints this line
  and then actually builds the object index from the configured path,
  reporting a real item count. Verified on this machine:
  ```
  VERBOSE: DirBase::rct2    : /home/edouard/Projects/Helter-Skelter/assets/gog/extracted/app
  VERBOSE: Checking if file exists: .../assets/gog/extracted/app/Data/g1.dat
  Building object index (2518 items)
  Finished building object index in 0.02 seconds.
  ```
  2,518 items is far more than the ~40 OpenRCT2-bundled JSON "shadow
  objects" alone, confirming the real GOG `ObjData`/`Data` content was read,
  not just OpenRCT2's own bundled replacement objects.

## Plugin directory

`~/.config/OpenRCT2/plugin/` — the `"plugin"` subdirectory name is defined
in `kDirectoryNamesOpenRCT2` — `src/openrct2/PlatformEnvironment.cpp:49`,
resolved under `DirBase::user`, which (per the note above) is the same
`~/.config/OpenRCT2` root as `config.ini` on Linux.

## Plugin API: hooks relevant to 0.1

`HookType` enum — `src/openrct2/scripting/HookEngine.h:28-49`. JS-facing
name strings — `src/openrct2/scripting/HookEngine.cpp:20-39`:

| Hook name | Fires |
|---|---|
| `action.query` | A game action was queried (validated, not executed). |
| `action.execute` | A game action was executed. |
| `interval.tick` | Every game tick. |
| `interval.day` | Every in-game day. |
| `map.save` | A save is about to happen — see the save-triggering gap below; this only lets a plugin *observe*, not *cause*, a save. |

## Game actions covering our five bounded commands

Scripting action-id strings and their `GameCommand` mapping —
`src/openrct2/scripting/ScriptEngine.cpp:1645-1727`. Argument shapes from
the shipped `doc/openrct2.d.ts` (packaged in the release tarball) and
cross-checked against each action's `AcceptParameters` in
`src/openrct2/actions/**/*.cpp`:

| Our command | Action id | Args (`doc/openrct2.d.ts`) | Source |
|---|---|---|---|
| `set_ride_price` | `"ridesetprice"` | `{ ride: number, price: number, isPrimaryPrice: boolean }` | `RideSetPriceAction.cpp:31-35` |
| `set_park_entry_fee` | `"parksetentrancefee"` | `{ value: number }` | `ParkSetEntranceFeeAction.cpp:25-27` |
| `hire_staff` | `"staffhire"` | `{ autoPosition: boolean, staffType: number, costumeIndex: number, staffOrders: number }` | `StaffHireNewAction.cpp:39-44` |
| `open_ride` / `close_ride` | `"ridesetstatus"` | `{ ride: number, status: number }` | `RideSetStatusAction.cpp:36-39` |

Both directions are exposed: `context.queryAction(action, args, cb)`
(validate only, no mutation) and `context.executeAction(action, args, cb)`
(mutate) — `src/openrct2/scripting/bindings/game/ScContext.hpp:293-307`.
This is a direct, engine-native match for the query/execute split
`docs/VISION.md` requires.

**`staffType` numeric mapping** (`doc/openrct2.d.ts` inline docs on
`StaffHireArgs`, matching `enum class StaffType : uint8_t` in
`src/openrct2/entity/Staff.h:35-42`): `0` = handyman, `1` = mechanic,
`2` = security, `3` = entertainer. Our protocol's `hire_staff.type` is a
string enum with the same four values in the same order — the bridge must
translate string → number when calling `executeAction`.

**`status` numeric mapping** (`doc/openrct2.d.ts` inline docs on
`RideSetStatusArgs`, matching `enum class RideStatus : uint8_t` in
`src/openrct2/ride/Ride.h:645-651`): `0` = closed, `1` = open, `2` =
testing, `3` = **simulating**.

**Result shape** — `interface GameActionResult { error?: number;
errorTitle?: string; errorMessage?: string; cost?: number; ... }`
(`doc/openrct2.d.ts:1612-1619`). Our `command.result` payload's
`engine_cost` maps from `.cost`; `engine_error` needs to combine `.error`
(numeric code) with `.errorTitle`/`.errorMessage` (the bridge must decide
how — not yet implemented, see Gaps).

### Known discrepancy with our existing protocol schema

`bridge/protocol/messages/observation_snapshot.schema.json`'s `ride.status`
enum is currently `["open", "closed", "testing"]` — it's missing
`"simulating"` (`RideStatus::simulating` = 3), which is a real ride state
the engine can report. This was flagged as a placeholder pending
verification in ADR-0002; this document is that verification. Not fixed
here (out of this task's scope — env/docs only) but should be addressed
before `observation.snapshot` is actually implemented against the live
API.

## TCP socket API and its localhost restriction

- `network.createSocket()` / `network.createListener()` expose a real TCP
  socket API to plugins — `src/openrct2/scripting/bindings/network/ScNetwork.cpp:300-322`.
  Gated only by the `DISABLE_NETWORK` CMake build option (default **off** —
  `CMakeLists.txt:110` — so present in standard/official builds); no
  plugin-type restriction gates it.
- **The engine itself enforces localhost-only**, not just convention:
  - `ScSocket::connect()`: `"For security reasons, only connecting to
    localhost is allowed."` if the target host isn't `localhost`,
    `127.0.0.1`, or `::` — `src/openrct2/scripting/bindings/network/ScSocket.hpp:89,299`.
  - `ScListener::listen()`: `"For security reasons, only binding to
    localhost is allowed."`, and even when allowed, hardcodes the bind
    address to `"127.0.0.1"` regardless of what's requested —
    `ScSocket.hpp:576,581`.
- This directly confirms our protocol's transport choice
  (`docs/PROTOCOL.md`: localhost TCP) is not just a convention we're
  imposing — it's the only thing the plugin API permits.

## Plugin type semantics (correction to our stub)

`bridge/openrct2-plugin/src/index.ts` currently declares `type: "remote"`.
Per `enum class PluginType` (`src/openrct2/scripting/Plugin.h:24-43`),
that's not the right semantic fit:
- `Local` — runs on servers/clients, no game-state impact, never uploaded
  to other clients.
- `Remote` — uploaded to other clients in multiplayer, with ability to
  modify game state in certain contexts. This is about **multiplayer
  distribution**, not single-player capability.
- `Intransient` — loads at game start and only unloads explicitly, rather
  than unloading on every park change.

A long-running bridge/telemetry plugin that must survive scenario/park
changes without reloading is a better match for **`intransient`** than
`remote`. Not changed here (out of this task's scope) — flagged for the
plugin implementation task.

## Headless-mode limitations

`gOpenRCT2Headless` gates dozens of call sites; the pattern (confirmed by
sampling, not an exhaustive list) is that headless mode skips anything
requiring a display or audio device:
- No window/renderer creation — `src/openrct2/Context.cpp:400,457` (`if
  (!gOpenRCT2Headless) { _uiContext->CreateWindow(); ... }`).
- No audio initialization — `src/openrct2/audio/Audio.cpp:72`.
- No title-sequence loading/intro scene — `src/openrct2/scenes/title/TitleScene.cpp:293`,
  `src/openrct2/scenes/intro/IntroScene.cpp:234`.
- The RCT2-path interactive prompt fallback is skipped (see above) —
  `Context.cpp:399-407`.
- Elevated-privilege and Wine warnings print to console instead of a
  message box — `Context.cpp:428-452`.

## Save-triggering from a plugin — GAP

**There is no scripting API function and no CLI subcommand to force an
immediate park save in `v0.5.3`.** Checked and ruled out:
- All `ScNetwork`/`ScSocket`/`ScConsole` scripting bindings — no `save`
  function exists (`console.executeLegacy` runs a legacy console command
  string, and no `"save"` legacy command exists anywhere in the codebase).
- `map.save` (`HookEngine.cpp:35`) is a hook a plugin can *subscribe to*
  (fired from `PrepareMapForSave()`, `src/openrct2/Game.cpp:792-803`) but
  never *trigger*.
- The `loadorquit` action (`GameCommand::LoadOrQuit`,
  `src/openrct2/actions/general/LoadOrQuitAction.h`) only opens/closes the
  interactive save-prompt **UI dialog** (`LoadOrQuitModes::openSavePrompt` /
  `closeSavePrompt`) — it doesn't write a file, and the dialog doesn't
  exist headless anyway.
- The `simulate <park> <ticks>` CLI command loads a park, runs ticks, and
  prints an entity checksum — it never writes a file
  (`src/openrct2/command_line/SimulateCommands.cpp:60-84`).
- The real save routine, `SaveGame()` (`src/openrct2/Game.cpp`), is
  internal-only, reached only via the autosave timer (`config.ini`'s
  `autosave`/`autosave_amount`, `Config.cpp:185-186`), the interactive
  save-prompt UI, or a network server's own periodic save
  (`src/openrct2/network/NetworkBase.cpp:2892`).

**Why this matters:** `docs/VISION.md`'s AFK-safety requirements call for
"snapshot before major action" and rollback. That isn't achievable from
the plugin (or CLI) alone today. It does **not** block 0.1's five bounded
actions specifically — none are destructive or hard to reverse in place
(price/staff/status changes), and the decision ledger (proposal →
authorization → execution → result) lives in PostgreSQL, independent of
`.park` file state. It does mean crash-recovery granularity for actual
game state is bounded by the autosave interval, not by our own action
log, until either a future OpenRCT2 version exposes a save function or
this project forks the engine to add one (an option `docs/VISION.md`
explicitly allows: "Fork the C++ engine only when plugin APIs are
genuinely insufficient").

## Gaps

- **No plugin/CLI-triggered save** (detailed above). Affects future
  snapshot/rollback design (0.2+ construction/demolition), not 0.1's five
  actions.
- **`engine_error` construction is unspecified.** `GameActionResult` gives
  a numeric `error` plus separate `errorTitle`/`errorMessage` strings; our
  protocol's `engine_error` is `{code, message}`. The exact mapping (e.g.
  does `code` become the stringified numeric error, does `message`
  concatenate title+message) isn't decided — deferred to the bridge
  implementation task.
- **`ride.status`/`weather` enum completeness.** `ride.status` is missing
  `simulating` (documented above as a known discrepancy, not fixed here).
  `weather`'s exact wire values were written speculatively in the original
  protocol task and have not yet been checked against
  `src/openrct2/climate/*` — still open.
- **Plugin bundling format assumption unverified against `doc/openrct2.d.ts`
  in detail** beyond the action/hook signatures checked here — the full
  `registerPlugin()` metadata shape (`bridge/openrct2-plugin/src/index.ts`)
  should be diffed against `doc/openrct2.d.ts` when the plugin is actually
  implemented, not assumed correct from this env-setup task.
