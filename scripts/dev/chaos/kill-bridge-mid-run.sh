#!/usr/bin/env bash
# Chaos test: kill -9 the bridge (openrct2-cli) mid-run. Asserts the
# orchestrator detects LOST via /health without crashing, and recovers to
# LIVE when a fresh bridge connects -- see docs/DECISIONS.md ADR-0006.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${SCRIPT_DIR}/lib.sh"

TIMESTAMP="$(date +%Y%m%dT%H%M%S)"
ORCH_LOG="${LOG_DIR}/kill-bridge-orchestrator-${TIMESTAMP}.log"
GAME_LOG="${LOG_DIR}/kill-bridge-game-${TIMESTAMP}.log"
GAME_LOG_2="${LOG_DIR}/kill-bridge-game-restart-${TIMESTAMP}.log"

ORCH_PID=""
GAME_PID=""
cleanup() {
    log "cleaning up..."
    [ -n "${GAME_PID}" ] && kill_if_running "${GAME_PID}"
    [ -n "${ORCH_PID}" ] && kill_if_running "${ORCH_PID}"
}
trap cleanup EXIT

build_stack

log "starting orchestrator (log: ${ORCH_LOG})"
ORCH_PID="$(start_orchestrator "${ORCH_LOG}")"
sleep 1

log "starting openrct2 headless (log: ${GAME_LOG})"
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG}")"

log "waiting for bridge state to reach 'live'..."
wait_for_health_field ".state" "live" 20 || fail "bridge never reached live"
pass "bridge reached live"

log "kill -9 the bridge (openrct2-cli pid ${GAME_PID})..."
kill_if_running "${GAME_PID}"
GAME_PID=""

log "waiting for orchestrator to detect 'lost' (LOST_AFTER=10s)..."
wait_for_health_field ".state" "lost" 20 || fail "orchestrator never detected lost"
pass "orchestrator detected lost without crashing"

health_after_kill="$(curl -s --max-time 2 http://127.0.0.1:8091/health)"
[ -n "${health_after_kill}" ] || fail "orchestrator /health stopped responding after the bridge was killed"
pass "orchestrator process survived the bridge crash (/health still responds)"

log "starting a fresh openrct2 headless instance (log: ${GAME_LOG_2})"
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG_2}")"

log "waiting for bridge state to recover to 'live'..."
wait_for_health_field ".state" "live" 30 || fail "bridge never recovered to live after reconnecting"
pass "bridge recovered to live after a fresh connection"

log "checking for duplicate idempotency_key violations in the orchestrator log..."
if grep -qi "idempotency_key.*duplicate\|violates unique constraint \"actions_idempotency_key_key\"" "${ORCH_LOG}"; then
    fail "found an idempotency_key conflict in the orchestrator log"
fi
pass "no idempotency_key conflicts"

pass "kill-bridge-mid-run: all assertions passed"
