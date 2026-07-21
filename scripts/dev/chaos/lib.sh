#!/usr/bin/env bash
# Shared helpers for scripts/dev/chaos/*.sh -- sourced, not run directly.
# See docs/DECISIONS.md ADR-0006's chaos-testing section.

set -euo pipefail

OPENRCT2_VERSION="v0.5.3"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OPENRCT2_BIN="${REPO_ROOT}/runtime/openrct2/${OPENRCT2_VERSION}/OpenRCT2/openrct2-cli"
PARK="${REPO_ROOT}/assets/scenarios/dev/dev-park.park"
PLUGIN_DIR="${HOME}/.config/OpenRCT2/plugin"
ORCH_BIN="${REPO_ROOT}/target/debug/orchestrator"
LOG_DIR="${REPO_ROOT}/logs/chaos"
export DATABASE_URL="${DATABASE_URL:-postgres://helterskelter:helterskelter@localhost:5433/helterskelter}"

mkdir -p "${LOG_DIR}"

log() { printf '[chaos] %s\n' "$*"; }
fail() { printf '[chaos] FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf '[chaos] PASS: %s\n' "$*"; }

# wait_for_health_field <jq-field-expression> <expected-value> [timeout-secs]
wait_for_health_field() {
    local field="$1" expected="$2" timeout="${3:-30}" waited=0 value
    while true; do
        value="$(curl -s --max-time 2 http://127.0.0.1:8091/health | jq -r "${field}" 2>/dev/null || echo "")"
        [ "${value}" = "${expected}" ] && return 0
        waited=$((waited + 1))
        [ "${waited}" -ge "${timeout}" ] && return 1
        sleep 1
    done
}

health_field() {
    curl -s --max-time 2 http://127.0.0.1:8091/health | jq -r "$1" 2>/dev/null || echo ""
}

build_stack() {
    log "bringing up PostgreSQL and applying migrations..."
    (cd "${REPO_ROOT}" && make db-up && DATABASE_URL="${DATABASE_URL}" make db-migrate) >/dev/null

    log "building bridge plugin..."
    (cd "${REPO_ROOT}/bridge/openrct2-plugin" && pnpm build) >/dev/null
    mkdir -p "${PLUGIN_DIR}"
    cp "${REPO_ROOT}/bridge/openrct2-plugin/dist/plugin.js" "${PLUGIN_DIR}/helter-skelter-bridge.js"

    log "building orchestrator..."
    (cd "${REPO_ROOT}" && DATABASE_URL="${DATABASE_URL}" cargo build -p orchestrator) >/dev/null
    [ -x "${ORCH_BIN}" ] || fail "orchestrator binary not found at ${ORCH_BIN}"
}

# start_orchestrator <log-file> -- prints the started PID on stdout.
start_orchestrator() {
    local log_file="$1"
    (cd "${REPO_ROOT}" && RUST_LOG=info DATABASE_URL="${DATABASE_URL}" "${ORCH_BIN}") >"${log_file}" 2>&1 &
    disown
    echo $!
}

# start_openrct2 <park> <log-file> -- prints the started PID on stdout.
start_openrct2() {
    local park="$1" log_file="$2"
    "${OPENRCT2_BIN}" "${park}" --headless --verbose >"${log_file}" 2>&1 &
    disown
    echo $!
}

kill_if_running() {
    local pid="$1"
    kill -9 "${pid}" 2>/dev/null || true
}
