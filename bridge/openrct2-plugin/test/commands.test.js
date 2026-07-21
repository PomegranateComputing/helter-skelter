// Unit tests for the pure parts of command execution: the numeric
// staffType mapping, the five bounded commands' mapping to their real
// game action id/args, and the engine-result -> ErrorInfo mapping. The
// idempotency cache and the actual query/execute calls depend on the
// `context` global (only present inside a running OpenRCT2 process) and
// are proven instead by the real end-to-end run in the PR description.
import { test } from "node:test";
import assert from "node:assert/strict";

import { staffTypeToNumber, toGameAction, toErrorInfo } from "../dist/pure.js";

test("staffTypeToNumber matches enum class StaffType in src/entity/Staff.h", () => {
  assert.equal(staffTypeToNumber("handyman"), 0);
  assert.equal(staffTypeToNumber("mechanic"), 1);
  assert.equal(staffTypeToNumber("security"), 2);
  assert.equal(staffTypeToNumber("entertainer"), 3);
});

test("toGameAction maps all five bounded commands to their real game action", () => {
  assert.deepEqual(
    toGameAction({ action: "set_ride_price", params: { ride_id: 3, price: 5 }, idempotency_key: "k", expiry_tick: 0 }),
    { action: "ridesetprice", args: { ride: 3, price: 5, isPrimaryPrice: true } },
  );

  assert.deepEqual(
    toGameAction({ action: "set_park_entry_fee", params: { price: 10 }, idempotency_key: "k", expiry_tick: 0 }),
    { action: "parksetentrancefee", args: { value: 10 } },
  );

  assert.deepEqual(
    toGameAction({ action: "hire_staff", params: { type: "mechanic" }, idempotency_key: "k", expiry_tick: 0 }),
    { action: "staffhire", args: { autoPosition: true, staffType: 1, costumeIndex: 0, staffOrders: 0 } },
  );

  assert.deepEqual(
    toGameAction({ action: "open_ride", params: { ride_id: 7 }, idempotency_key: "k", expiry_tick: 0 }),
    { action: "ridesetstatus", args: { ride: 7, status: 1 } },
  );

  assert.deepEqual(
    toGameAction({ action: "close_ride", params: { ride_id: 7 }, idempotency_key: "k", expiry_tick: 0 }),
    { action: "ridesetstatus", args: { ride: 7, status: 0 } },
  );
});

test("toErrorInfo returns null for a successful result", () => {
  assert.equal(toErrorInfo({ cost: 0 }), null);
  assert.equal(toErrorInfo({ cost: 0, error: 0 }), null);
});

test("toErrorInfo builds a structured ErrorInfo for a failed result", () => {
  assert.deepEqual(toErrorInfo({ error: 5, errorMessage: "Ride not found" }), {
    code: "5",
    message: "Ride not found",
  });
  assert.deepEqual(toErrorInfo({ error: 5, errorTitle: "Can't set ride price...", errorMessage: undefined }), {
    code: "5",
    message: "Can't set ride price...",
  });
});
