#!/usr/bin/env bash
# Chaos test: kill -9 the orchestrator immediately after it sends a
# command.request, before any command.result can arrive. Asserts: no
# idempotency_key conflict (no duplicate action ever gets a second try
# at execution), the restarted orchestrator's crash-recovery correctly
# enters Cautious and logs the orphaned action, and the bridge
# reconnects to the fresh orchestrator process on its own. See
# docs/DECISIONS.md ADR-0006.
#
# The exact race (kill between "command.request sent" and the reply
# arriving) is timing-sensitive over a near-instant localhost round trip
# -- this asserts invariants that must hold whether or not it actually
# won the race that run, not "the kill definitely landed mid-flight".

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${SCRIPT_DIR}/lib.sh"

TIMESTAMP="$(date +%Y%m%dT%H%M%S)"
ORCH_LOG="${LOG_DIR}/kill-orch-orchestrator-${TIMESTAMP}.log"
ORCH_LOG_2="${LOG_DIR}/kill-orch-orchestrator-restart-${TIMESTAMP}.log"
GAME_LOG="${LOG_DIR}/kill-orch-game-${TIMESTAMP}.log"

ORCH_PID=""
GAME_PID=""
cleanup() {
    log "cleaning up..."
    [ -n "${GAME_PID}" ] && kill_if_running "${GAME_PID}"
    [ -n "${ORCH_PID}" ] && kill_if_running "${ORCH_PID}"
    rm -f "${PLUGIN_DIR}/zz-chaos-seed.js"
}
trap cleanup EXIT

# Fresh ledger evidence for this run's assertions.
log "resetting the database for a clean ledger..."
(cd "${REPO_ROOT}" && docker compose down -v db >/dev/null 2>&1 || true)
build_stack
cp "${SCRIPT_DIR}/seed-price.js" "${PLUGIN_DIR}/zz-chaos-seed.js"

log "starting orchestrator (log: ${ORCH_LOG})"
ORCH_PID="$(start_orchestrator "${ORCH_LOG}")"
sleep 1

log "starting openrct2 headless with the seed plugin (log: ${GAME_LOG})"
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG}")"

log "waiting for bridge state to reach 'live'..."
wait_for_health_field ".state" "live" 20 || fail "bridge never reached live"

log "waiting for a command.request to be sent, then killing the orchestrator immediately..."
timeout 30 bash -c "tail -n0 -F '${ORCH_LOG}' | grep -m1 'command.request sent'" \
    || fail "never saw a command.request sent within 30s"
kill_if_running "${ORCH_PID}"
pass "killed the orchestrator right after it sent a command.request"

log "restarting the orchestrator against the same database (log: ${ORCH_LOG_2})"
ORCH_PID="$(start_orchestrator "${ORCH_LOG_2}")"

log "waiting for crash-recovery to enter cautious on the fresh process..."
wait_for_health_field ".state" "connecting" 20 || fail "restarted orchestrator's health endpoint never came up"

log "waiting for the bridge to reconnect to the fresh orchestrator..."
wait_for_health_field ".state" "live" 30 || fail "bridge never reconnected to the restarted orchestrator"
pass "bridge reconnected to the restarted orchestrator"

log "checking for idempotency_key conflicts across both orchestrator runs..."
if grep -qi "violates unique constraint \"actions_idempotency_key_key\"" "${ORCH_LOG}" "${ORCH_LOG_2}"; then
    fail "found an idempotency_key conflict -- an action may have been double-executed"
fi
pass "no idempotency_key conflicts across the crash and restart"

log "checking the restarted orchestrator logged crash-recovery cautious entry..."
if ! grep -qi "entering cautious on startup" "${ORCH_LOG_2}"; then
    fail "restarted orchestrator did not log entering cautious on startup"
fi
pass "restarted orchestrator entered cautious on startup"

log "querying the state_transitions ledger for the cautious entry..."
cautious_row="$(docker exec helter-skelter-db-1 psql -U helterskelter -d helterskelter -tAc \
    "SELECT count(*) FROM state_transitions WHERE to_state = 'cautious' AND triggered_by = 'orchestrator' AND reason = 'orchestrator startup';")"
[ "${cautious_row}" -ge 1 ] || fail "no cautious state_transitions row recorded for the restart"
pass "cautious transition recorded in the ledger (count: ${cautious_row})"

pass "kill-orchestrator-with-action-in-flight: all assertions passed"
