#!/usr/bin/env bash
# Always fails -- stands in for scripts/dev/snapshot.sh when no autosave
# exists yet, exercising the "can't ensure a snapshot" path (rejected
# authorization, no governor side effects) without needing a real
# OpenRCT2 process. See rollback.rs's snapshot_failure_does_not_consume_the_ride_cooldown.
echo "no autosave found (simulated failure)" >&2
exit 1
