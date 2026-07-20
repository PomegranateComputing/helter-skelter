#!/usr/bin/env bash
set -euo pipefail

# Launches the pinned OpenRCT2 (scripts/bootstrap/setup-openrct2.sh) headless
# with the dev park (assets/scenarios/dev/dev-park.park), logging to logs/.
#
# Plugins are loaded from the default plugin directory,
# ~/.config/OpenRCT2/plugin/ (see docs/OPENRCT2_INTEGRATION.md) -- there is
# no --plugin-dir override flag in v0.5.3, so a plugin under development
# must be built to (or symlinked into) that directory to be picked up.
#
# Runs until killed (Ctrl-C) or OpenRCT2 exits on its own -- for a bounded
# demo run, wrap the invocation in `timeout`, e.g.:
#   timeout 30 scripts/dev/run-headless.sh

OPENRCT2_VERSION="v0.5.3"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="${REPO_ROOT}/runtime/openrct2/${OPENRCT2_VERSION}/OpenRCT2/openrct2-cli"
PARK="${REPO_ROOT}/assets/scenarios/dev/dev-park.park"
LOG_DIR="${REPO_ROOT}/logs"
LOG_FILE="${LOG_DIR}/openrct2-headless-$(date +%Y%m%dT%H%M%S).log"

log() { printf '[run-headless] %s\n' "$*"; }
fail() { printf '[run-headless] ERROR: %s\n' "$*" >&2; exit 1; }

[ -x "${BIN}" ] || fail "OpenRCT2 not installed. Run scripts/bootstrap/setup-openrct2.sh first."
[ -f "${PARK}" ] || fail "Dev park not found at ${PARK} (see assets/scenarios/dev/README.md)."

mkdir -p "${LOG_DIR}"

log "Binary:  ${BIN}"
log "Park:    ${PARK}"
log "Plugins: ~/.config/OpenRCT2/plugin/ (default)"
log "Logging: ${LOG_FILE}"

"${BIN}" "${PARK}" --headless --verbose "$@" 2>&1 | tee "${LOG_FILE}"
