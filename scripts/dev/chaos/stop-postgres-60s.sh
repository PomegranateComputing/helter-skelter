#!/usr/bin/env bash
# Chaos test: stop PostgreSQL for 60+ seconds while the stack keeps
# running. Asserts the orchestrator degrades to db_state=cautious without
# crashing or dropping the bridge connection, then recovers to
# db_state=connected once the database comes back, with buffered
# observations eventually flushing. See docs/DECISIONS.md ADR-0006 and
# ADR-0003 (the underlying db_state degradation this re-verifies in the
# new chaos-test suite).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${SCRIPT_DIR}/lib.sh"

TIMESTAMP="$(date +%Y%m%dT%H%M%S)"
ORCH_LOG="${LOG_DIR}/stop-pg-orchestrator-${TIMESTAMP}.log"
GAME_LOG="${LOG_DIR}/stop-pg-game-${TIMESTAMP}.log"

ORCH_PID=""
GAME_PID=""
cleanup() {
    log "cleaning up..."
    [ -n "${GAME_PID}" ] && kill_if_running "${GAME_PID}"
    [ -n "${ORCH_PID}" ] && kill_if_running "${ORCH_PID}"
    docker start helter-skelter-db-1 >/dev/null 2>&1 || true
}
trap cleanup EXIT

build_stack

log "starting orchestrator (log: ${ORCH_LOG})"
ORCH_PID="$(start_orchestrator "${ORCH_LOG}")"
sleep 1

log "starting openrct2 headless (log: ${GAME_LOG})"
GAME_PID="$(start_openrct2 "${PARK}" "${GAME_LOG}")"

log "waiting for bridge state to reach 'live' with db_state=connected..."
wait_for_health_field ".state" "live" 20 || fail "bridge never reached live"
wait_for_health_field ".db_state" "connected" 10 || fail "db_state never reached connected"

observations_before="$(docker exec helter-skelter-db-1 psql -U helterskelter -d helterskelter -tAc "SELECT count(*) FROM observations;")"
log "observations recorded before outage: ${observations_before}"

log "stopping PostgreSQL..."
docker stop helter-skelter-db-1 >/dev/null
outage_start="$(date +%s)"

log "polling for 60s to confirm the orchestrator degrades but survives..."
survived=true
saw_cautious=false
for _ in $(seq 1 12); do
    sleep 5
    if ! curl -s --max-time 2 http://127.0.0.1:8091/health >/dev/null; then
        survived=false
        break
    fi
    db_state="$(health_field ".db_state")"
    [ "${db_state}" = "cautious" ] && saw_cautious=true
done
elapsed="$(($(date +%s) - outage_start))"
log "outage lasted ${elapsed}s so far"

${survived} || fail "orchestrator /health stopped responding during the outage"
pass "orchestrator survived a ${elapsed}s database outage without crashing"

${saw_cautious} || fail "orchestrator never reported db_state=cautious during the outage"
pass "orchestrator correctly reported db_state=cautious during the outage"

bridge_state_during_outage="$(health_field ".state")"
[ "${bridge_state_during_outage}" = "live" ] || fail "bridge connection was dropped during the DB outage (state: ${bridge_state_during_outage})"
pass "bridge connection stayed live throughout the DB outage"

log "starting PostgreSQL again..."
docker start helter-skelter-db-1 >/dev/null

log "waiting for db_state to recover to 'connected'..."
wait_for_health_field ".db_state" "connected" 60 || fail "db_state never recovered to connected"
pass "db_state recovered to connected"

log "waiting for buffered observations to flush..."
flushed=false
for _ in $(seq 1 20); do
    observations_after="$(docker exec helter-skelter-db-1 psql -U helterskelter -d helterskelter -tAc "SELECT count(*) FROM observations;" 2>/dev/null || echo "${observations_before}")"
    if [ "${observations_after}" -gt "${observations_before}" ]; then
        flushed=true
        break
    fi
    sleep 2
done
${flushed} || fail "no new observations were persisted after the database recovered"
pass "buffered observations flushed after recovery (${observations_before} -> ${observations_after})"

pass "stop-postgres-60s: all assertions passed"
