#!/usr/bin/env bash
set -euo pipefail

# Host-side stand-in for an on-demand park save. Per
# docs/OPENRCT2_INTEGRATION.md's "Save-triggering from a plugin" GAP,
# v0.5.3 has no scripting API function and no CLI subcommand to force an
# immediate save -- the only save the engine produces on its own is the
# periodic autosave (config.ini's `autosave`/`autosave_amount`, finest
# granularity AUTOSAVE_EVERY_MINUTE, real wall-clock time, not on demand
# and not tick-aligned).
#
# So this script does not *trigger* a save -- it copies whichever
# autosave the engine has already written most recently. That means the
# copied snapshot's true in-game tick can lag the `tick` argument by up
# to the autosave interval; core/orchestrator/src/snapshot.rs records the
# orchestrator's own tick at copy time as a label, not a guarantee of
# exact correspondence. See docs/DECISIONS.md ADR-0005.
#
# Usage: snapshot.sh <dest-dir> <tick>
# Prints the resulting file's absolute path to stdout on success.

AUTOSAVE_DIR="${OPENRCT2_AUTOSAVE_DIR:-$HOME/.config/OpenRCT2/save/autosave}"

DEST_DIR="${1:?usage: snapshot.sh <dest-dir> <tick>}"
TICK="${2:?usage: snapshot.sh <dest-dir> <tick>}"

latest="$(ls -t "${AUTOSAVE_DIR}"/*.park 2>/dev/null | head -1 || true)"
if [ -z "${latest}" ]; then
    echo "no autosave found in ${AUTOSAVE_DIR} -- has the engine been running long enough for its first autosave?" >&2
    exit 1
fi

mkdir -p "${DEST_DIR}"
dest="${DEST_DIR}/${TICK}.park"
cp "${latest}" "${dest}"
echo "${dest}"
