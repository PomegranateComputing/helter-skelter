#!/usr/bin/env bash
set -euo pipefail

# Builds and runs the full 0.1 stack for real: core/orchestrator (TCP
# server + /health) and headless OpenRCT2 with the dev park and the bridge
# plugin installed, logging both to logs/ and tailing them together.
#
# Run until killed (Ctrl-C). Check liveness separately with:
#   curl -s http://127.0.0.1:8091/health | jq
#
# To roll back to a snapshot: stop this script, run
#   DATABASE_URL=... target/debug/orchestrator rollback --to <snapshot_id>
# then re-run this script -- it loads runtime/current-park.park (which
# the rollback just restored) instead of the default dev park if present.

OPENRCT2_VERSION="v0.5.3"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG_DIR="${REPO_ROOT}/logs"
TIMESTAMP="$(date +%Y%m%dT%H%M%S)"
ORCH_LOG="${LOG_DIR}/orchestrator-${TIMESTAMP}.log"
GAME_LOG="${LOG_DIR}/openrct2-headless-${TIMESTAMP}.log"

OPENRCT2_BIN="${REPO_ROOT}/runtime/openrct2/${OPENRCT2_VERSION}/OpenRCT2/openrct2-cli"
PARK_DEFAULT="${REPO_ROOT}/assets/scenarios/dev/dev-park.park"
PARK_CURRENT="${REPO_ROOT}/runtime/current-park.park"
PLUGIN_DIR="${HOME}/.config/OpenRCT2/plugin"
export DATABASE_URL="${DATABASE_URL:-postgres://helterskelter:helterskelter@localhost:5433/helterskelter}"

log() { printf '[run-stack] %s\n' "$*"; }
fail() { printf '[run-stack] ERROR: %s\n' "$*" >&2; exit 1; }

[ -x "${OPENRCT2_BIN}" ] || fail "OpenRCT2 not installed. Run scripts/bootstrap/setup-openrct2.sh first."

# `orchestrator rollback --to <id>` restores a snapshot to PARK_CURRENT --
# prefer it when present so a rollback actually takes effect on the next
# start, per docs/DECISIONS.md ADR-0005.
if [ -f "${PARK_CURRENT}" ]; then
    PARK="${PARK_CURRENT}"
    log "Using restored park: ${PARK} (runtime/current-park.park exists -- remove it to fall back to the default dev park)"
else
    PARK="${PARK_DEFAULT}"
fi
[ -f "${PARK}" ] || fail "Dev park not found at ${PARK} (see assets/scenarios/dev/README.md)."

log "Bringing up PostgreSQL and applying migrations..."
(cd "${REPO_ROOT}" && make db-up && DATABASE_URL="${DATABASE_URL}" make db-migrate)

log "Building bridge plugin..."
(cd "${REPO_ROOT}/bridge/openrct2-plugin" && pnpm build)
mkdir -p "${PLUGIN_DIR}"
cp "${REPO_ROOT}/bridge/openrct2-plugin/dist/plugin.js" "${PLUGIN_DIR}/helter-skelter-bridge.js"

log "Building orchestrator..."
(cd "${REPO_ROOT}" && cargo build -p orchestrator)
ORCH_BIN="${REPO_ROOT}/target/debug/orchestrator"
[ -x "${ORCH_BIN}" ] || fail "orchestrator binary not found at ${ORCH_BIN}"

mkdir -p "${LOG_DIR}"

ORCH_PID=""
GAME_PID=""
cleanup() {
    log "stopping..."
    [ -n "${GAME_PID}" ] && kill "${GAME_PID}" 2>/dev/null || true
    [ -n "${ORCH_PID}" ] && kill "${ORCH_PID}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

log "Starting orchestrator, logging to ${ORCH_LOG}"
(cd "${REPO_ROOT}" && RUST_LOG=info DATABASE_URL="${DATABASE_URL}" "${ORCH_BIN}") >"${ORCH_LOG}" 2>&1 &
ORCH_PID=$!

# Give the TCP/health listeners a moment to bind before the bridge tries
# to connect.
sleep 1

log "Starting OpenRCT2 headless with the dev park, logging to ${GAME_LOG}"
"${OPENRCT2_BIN}" "${PARK}" --headless --verbose >"${GAME_LOG}" 2>&1 &
GAME_PID=$!

log "orchestrator pid=${ORCH_PID}  openrct2 pid=${GAME_PID}"
log "health: curl -s http://127.0.0.1:8091/health"
log "tailing logs (Ctrl-C to stop)..."
tail -f "${ORCH_LOG}" "${GAME_LOG}"
