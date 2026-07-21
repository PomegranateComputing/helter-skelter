#!/usr/bin/env bash
set -euo pipefail

# Stand-in for scripts/dev/snapshot.sh used by tests: no real OpenRCT2
# process is running to have produced a real autosave, so this just
# creates an empty placeholder file at the expected location and prints
# its path -- tests assert against the snapshots/rollbacks ledger rows,
# never against real park file contents.
#
# Usage: fake-snapshot.sh <dest-dir> <tick>

DEST_DIR="${1:?usage: fake-snapshot.sh <dest-dir> <tick>}"
TICK="${2:?usage: fake-snapshot.sh <dest-dir> <tick>}"

mkdir -p "${DEST_DIR}"
dest="${DEST_DIR}/${TICK}.park"
: > "${dest}"
echo "${dest}"
