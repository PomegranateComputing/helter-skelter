/**
 * Executes an authorized command.request with a query/execute split:
 * context.queryAction() validates without mutating game state; only if
 * that succeeds does context.executeAction() actually mutate it. Replaying
 * the same idempotency_key returns the cached result instead of
 * re-invoking either -- the orchestrator already decided to authorize
 * this exact command once; re-running it on a retry/duplicate delivery
 * would double-apply the effect.
 *
 * "Orchestrator confirmation" for the execute half is the command.request
 * itself: it only exists on the wire because core/governor already
 * authorized it (see docs/PROTOCOL.md) -- there is no separate wire
 * round-trip for query-then-confirm-then-execute in this milestone.
 */
import type { CommandRequest, CommandResult, ErrorInfo } from "./protocol";

export interface GameActionResult {
  error?: number;
  errorTitle?: string;
  errorMessage?: string;
  cost?: number;
}

export interface GameAction {
  action: string;
  args: object;
}

const idempotencyCache = new Map<string, CommandResult>();

function queryAction(action: string, args: object): Promise<GameActionResult> {
  return new Promise((resolve) => {
    context.queryAction(action, args, (result) => resolve(result));
  });
}

function executeAction(action: string, args: object): Promise<GameActionResult> {
  return new Promise((resolve) => {
    context.executeAction(action, args, (result) => resolve(result));
  });
}

// Numeric mapping confirmed against doc/openrct2.d.ts's StaffHireArgs and
// enum class StaffType in src/entity/Staff.h -- see
// docs/OPENRCT2_INTEGRATION.md.
export function staffTypeToNumber(type: string): number {
  switch (type) {
    case "handyman":
      return 0;
    case "mechanic":
      return 1;
    case "security":
      return 2;
    case "entertainer":
      return 3;
    default:
      return 0;
  }
}

// Maps our five bounded commands to their real game action id and args --
// see docs/OPENRCT2_INTEGRATION.md's "Game actions covering our five
// bounded commands" table.
export function toGameAction(request: CommandRequest): GameAction {
  switch (request.action) {
    case "set_ride_price":
      return {
        action: "ridesetprice",
        args: { ride: request.params.ride_id, price: request.params.price, isPrimaryPrice: true },
      };
    case "set_park_entry_fee":
      return { action: "parksetentrancefee", args: { value: request.params.price } };
    case "hire_staff":
      return {
        action: "staffhire",
        args: {
          autoPosition: true,
          staffType: staffTypeToNumber(request.params.type),
          costumeIndex: 0,
          staffOrders: 0,
        },
      };
    case "open_ride":
      return { action: "ridesetstatus", args: { ride: request.params.ride_id, status: 1 } };
    case "close_ride":
      return { action: "ridesetstatus", args: { ride: request.params.ride_id, status: 0 } };
  }
}

export function toErrorInfo(result: GameActionResult): ErrorInfo | null {
  if (result.error === undefined || result.error === 0) {
    return null;
  }
  return { code: String(result.error), message: result.errorMessage ?? result.errorTitle ?? "unknown engine error" };
}

export async function handleCommandRequest(request: CommandRequest): Promise<CommandResult> {
  const cached = idempotencyCache.get(request.idempotency_key);
  if (cached) {
    console.log(`[bridge] replayed idempotency_key ${request.idempotency_key}, returning recorded result`);
    return cached;
  }

  const { action, args } = toGameAction(request);

  const queryResult = await queryAction(action, args);
  const queryError = toErrorInfo(queryResult);
  if (queryError) {
    const result: CommandResult = { engine_cost: null, engine_error: queryError };
    idempotencyCache.set(request.idempotency_key, result);
    return result;
  }

  const executeResult = await executeAction(action, args);
  const executeError = toErrorInfo(executeResult);
  const result: CommandResult = {
    engine_cost: executeError ? null : (executeResult.cost ?? 0),
    engine_error: executeError,
  };
  idempotencyCache.set(request.idempotency_key, result);
  return result;
}
