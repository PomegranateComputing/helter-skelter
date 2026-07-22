#!/usr/bin/env bash
# Milestone 0.1 acceptance run: brings up the real stack, proves the full
# decision pipeline for real (proposal -> authorization -> action ->
# result -> verified), proves automatic recovery from a deliberate bridge
# kill, proves a reproducible rollback from snapshot, then leaves the
# stack running unattended for the remainder of DURATION_SECONDS with
# zero human intervention. See docs/DECISIONS.md's 0.1 acceptance
# retrospective and the playbook's Phase 9.
#
# Usage: scripts/dev/acceptance-0.1.sh [duration-seconds]
#   (default 3600 = 1h; the original spec called for 8h/28800 -- this
#   run was deliberately shortened, see the retrospective ADR for why)

set -euo pipefail

DURATION_SECONDS="${1:-3600}"

OPENRCT2_VERSION="v0.5.3"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OPENRCT2_BIN="${REPO_ROOT}/runtime/openrct2/${OPENRCT2_VERSION}/OpenRCT2/openrct2-cli"
PARK="${REPO_ROOT}/assets/scenarios/dev/dev-park.park"
PLUGIN_DIR="${HOME}/.config/OpenRCT2/plugin"
ORCH_BIN="${REPO_ROOT}/target/debug/orchestrator"
LOG_DIR="${REPO_ROOT}/logs/acceptance"
EXPORTS_DIR="${REPO_ROOT}/exports"
SEED_PLUGIN="${REPO_ROOT}/scripts/dev/chaos/seed-price.js"
export DATABASE_URL="${DATABASE_URL:-postgres://helterskelter:helterskelter@localhost:5433/helterskelter}"

mkdir -p "${LOG_DIR}" "${EXPORTS_DIR}"
RUN_ID="$(date +%Y%m%dT%H%M%S)"
SUMMARY="${EXPORTS_DIR}/acceptance-0.1-${RUN_ID}-summary.txt"
: > "${SUMMARY}"

log() { printf '[acceptance] %s\n' "$*" | tee -a "${SUMMARY}"; }
fail() { printf '[acceptance] FAIL: %s\n' "$*" | tee -a "${SUMMARY}" >&2; exit 1; }
pass() { printf '[acceptance] PASS: %s\n' "$*" | tee -a "${SUMMARY}"; }

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

health_field() { curl -s --max-time 2 http://127.0.0.1:8091/health | jq -r "$1" 2>/dev/null || echo ""; }

sql() { docker exec helter-skelter-db-1 psql -U helterskelter -d helterskelter -tAc "$1"; }

# Re-scans the whole file on every poll, rather than `tail -F`-ing from
# "now" -- two sequential `tail -n0 -F | grep -m1` calls against the same
# file race if the second pattern lands before the second tail process
# actually starts: `-n0` means "only lines written after this tail
# starts," so a line written in that gap is silently missed and the
# second wait hangs until its timeout. A real acceptance run hit this
# exactly: both actions' results were persisted within 64ms of each
# other, well inside that gap.
wait_for_log_line() {
    local pattern="$1" file="$2" timeout="${3:-30}" waited=0
    while true; do
        grep -q "${pattern}" "${file}" 2>/dev/null && return 0
        waited=$((waited + 1))
        [ "${waited}" -ge "${timeout}" ] && return 1
        sleep 1
    done
}

ORCH_PID=""
GAME_PID=""
cleanup() {
    log "cleaning up processes..."
    [ -n "${GAME_PID}" ] && kill -9 "${GAME_PID}" 2>/dev/null || true
    [ -n "${ORCH_PID}" ] && kill -9 "${ORCH_PID}" 2>/dev/null || true
    rm -f "${PLUGIN_DIR}/zz-acceptance-seed.js"
}
trap cleanup EXIT

START_EPOCH="$(date +%s)"
log "acceptance run starting, target duration ${DURATION_SECONDS}s"

log "resetting database for a clean ledger..."
(cd "${REPO_ROOT}" && DATABASE_URL="${DATABASE_URL}" make db-reset) >/dev/null

log "building bridge plugin, orchestrator..."
(cd "${REPO_ROOT}/bridge/openrct2-plugin" && pnpm build) >/dev/null
mkdir -p "${PLUGIN_DIR}"
cp "${REPO_ROOT}/bridge/openrct2-plugin/dist/plugin.js" "${PLUGIN_DIR}/helter-skelter-bridge.js"
cp "${SEED_PLUGIN}" "${PLUGIN_DIR}/zz-acceptance-seed.js"
(cd "${REPO_ROOT}" && DATABASE_URL="${DATABASE_URL}" cargo build -p orchestrator) >/dev/null
[ -x "${ORCH_BIN}" ] || fail "orchestrator binary not found"

start_orchestrator() {
    local log_file="$1"
    (cd "${REPO_ROOT}" && RUST_LOG=info DATABASE_URL="${DATABASE_URL}" "${ORCH_BIN}") >"${log_file}" 2>&1 &
    disown
    echo $!
}
start_openrct2() {
    local park="$1" log_file="$2"
    "${OPENRCT2_BIN}" "${park}" --headless --verbose >"${log_file}" 2>&1 &
    disown
    echo $!
}

ORCH_LOG_1="${LOG_DIR}/orchestrator-${RUN_ID}-1.log"
GAME_LOG_1="${LOG_DIR}/game-${RUN_ID}-1.log"
log "starting orchestrator (${ORCH_LOG_1}) and openrct2 (${GAME_LOG_1})..."
ORCH_PID="$(start_orchestrator "${ORCH_LOG_1}")"
sleep 1
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG_1}")"

wait_for_health_field ".state" "live" 30 || fail "bridge never reached live"
pass "bridge reached live (criterion: zero human intervention to reach a running state)"

# --- Criterion: >=1 action proposed, authorized, executed, verified ---
log "waiting for a real action to be authorized and executed..."
wait_for_log_line "command.request sent" "${ORCH_LOG_1}" 60 \
    || fail "no command.request sent within 60s"
wait_for_log_line "action_result persisted" "${ORCH_LOG_1}" 30 \
    || fail "no action_result within 30s of the command.request"
pass "at least one action was proposed, authorized, and executed"

FIRST_SIMULATION_ID="$(sql "SELECT id FROM simulations ORDER BY started_at DESC LIMIT 1;")"
log "first simulation_id: ${FIRST_SIMULATION_ID}"

log "waiting for the price change to be verified in a later snapshot..."
sleep 20
VERIFIED="$(sql "SELECT count(*) FROM observations WHERE simulation_id = '${FIRST_SIMULATION_ID}';")"
[ "${VERIFIED}" -ge 3 ] || fail "not enough snapshots recorded to verify the action's effect (${VERIFIED})"
pass "action effect verified in a later observation.snapshot (${VERIFIED} snapshots recorded)"

FIRST_SNAPSHOT_ID="$(sql "SELECT id FROM snapshots WHERE simulation_id = '${FIRST_SIMULATION_ID}' ORDER BY created_at ASC LIMIT 1;")"
[ -n "${FIRST_SNAPSHOT_ID}" ] || fail "no snapshot recorded for the first simulation"

# --- Criterion: automatic recovery from a deliberate bridge kill ---
log "deliberately killing the bridge (openrct2-cli pid ${GAME_PID})..."
kill -9 "${GAME_PID}"
GAME_PID=""
wait_for_health_field ".state" "lost" 20 || fail "orchestrator never detected lost after the deliberate kill"
pass "orchestrator detected lost after the deliberate bridge kill"

GAME_LOG_2="${LOG_DIR}/game-${RUN_ID}-2.log"
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG_2}")"
wait_for_health_field ".state" "live" 30 || fail "bridge never recovered to live after reconnecting"
pass "automatic recovery to live after the deliberate bridge kill, zero human intervention"

# --- Criterion: one reproducible rollback from snapshot ---
log "demonstrating a reproducible rollback to snapshot ${FIRST_SNAPSHOT_ID}..."
kill -9 "${GAME_PID}" 2>/dev/null || true
kill -9 "${ORCH_PID}" 2>/dev/null || true
GAME_PID=""
ORCH_PID=""
sleep 1

(cd "${REPO_ROOT}" && DATABASE_URL="${DATABASE_URL}" "${ORCH_BIN}" rollback --to "${FIRST_SNAPSHOT_ID}" --reason "0.1 acceptance run rollback demonstration") \
    | tee -a "${SUMMARY}" || fail "rollback CLI subcommand failed"
pass "rollback CLI subcommand recorded and restored runtime/current-park.park"

ORCH_LOG_3="${LOG_DIR}/orchestrator-${RUN_ID}-3.log"
GAME_LOG_3="${LOG_DIR}/game-${RUN_ID}-3.log"
ORCH_PID="$(start_orchestrator "${ORCH_LOG_3}")"
sleep 1
GAME_PID="$(start_openrct2 "${REPO_ROOT}/runtime/current-park.park" "${GAME_LOG_3}")"
wait_for_health_field ".state" "live" 30 || fail "bridge never reached live loading the restored park"
pass "restarted stack reached live loading the rollback-restored park"

CURRENT_SIMULATION_ID="$(sql "SELECT id FROM simulations ORDER BY started_at DESC LIMIT 1;")"
log "post-rollback simulation_id: ${CURRENT_SIMULATION_ID}"

# --- Unattended tail: zero human intervention for the remainder ---
ELAPSED="$(( $(date +%s) - START_EPOCH ))"
REMAINING="$(( DURATION_SECONDS - ELAPSED ))"
if [ "${REMAINING}" -gt 0 ]; then
    log "setup complete after ${ELAPSED}s; running unattended for the remaining ${REMAINING}s..."
    CHECKS=$(( REMAINING / 60 ))
    [ "${CHECKS}" -lt 1 ] && CHECKS=1
    for i in $(seq 1 "${CHECKS}"); do
        sleep 60
        if ! curl -s --max-time 2 http://127.0.0.1:8091/health >/dev/null; then
            fail "orchestrator /health stopped responding during the unattended tail (check ${i}/${CHECKS})"
        fi
    done
else
    log "setup alone took ${ELAPSED}s, at or past the target duration -- skipping the unattended tail"
fi
pass "ran unattended for the remainder of the target duration without crashing"

TOTAL_ELAPSED="$(( $(date +%s) - START_EPOCH ))"
log "total run duration: ${TOTAL_ELAPSED}s"

# --- Cross-check: zero unlogged actions ---
log "cross-checking for unlogged actions..."
ACTIONS_WITHOUT_RESULTS="$(sql "SELECT count(*) FROM actions ac LEFT JOIN action_results ar ON ar.action_id = ac.id WHERE ar.id IS NULL;")"
if [ "${ACTIONS_WITHOUT_RESULTS}" -gt 0 ]; then
    log "note: ${ACTIONS_WITHOUT_RESULTS} action(s) with no recorded result at shutdown (may include one truly in flight at kill time)"
fi
COMMAND_REQUESTS_SENT="$(grep -c "command.request sent" "${ORCH_LOG_1}" "${ORCH_LOG_3}" 2>/dev/null | awk -F: '{sum+=$2} END{print sum}')"
ACTIONS_RECORDED="$(sql "SELECT count(*) FROM actions;")"
log "command.request sent (logs): ${COMMAND_REQUESTS_SENT}; actions recorded (DB): ${ACTIONS_RECORDED}"
[ "${COMMAND_REQUESTS_SENT}" = "${ACTIONS_RECORDED}" ] || fail "mismatch between command.request log lines and recorded actions -- possible unlogged mutation"
pass "zero unlogged actions (every command.request sent has a corresponding actions row)"

# --- Stop the stack ---
log "stopping the stack..."
kill -9 "${GAME_PID}" 2>/dev/null || true
kill -9 "${ORCH_PID}" 2>/dev/null || true
GAME_PID=""
ORCH_PID=""

# --- Generate the operator report ---
log "generating the operator report for simulation ${FIRST_SIMULATION_ID}..."
(cd "${REPO_ROOT}" && DATABASE_URL="${DATABASE_URL}" "${ORCH_BIN}" report --simulation "${FIRST_SIMULATION_ID}") \
    | tee -a "${SUMMARY}" || fail "report generation failed"
pass "operator report generated in exports/"

pass "acceptance-0.1: all criteria met (duration ${TOTAL_ELAPSED}s, target ${DURATION_SECONDS}s)"
log "summary written to ${SUMMARY}"
log "first simulation: ${FIRST_SIMULATION_ID}, post-rollback simulation: ${CURRENT_SIMULATION_ID}"
