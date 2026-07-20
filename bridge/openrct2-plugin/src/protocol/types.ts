/**
 * Versioned message protocol shared with core/common. Keep in lockstep with
 * bridge/protocol/*.schema.json and core/common/src/protocol/*.rs -- see
 * docs/PROTOCOL.md. Compile-time conformance against the canonical fixtures
 * in bridge/messages/fixtures/ is checked in ./fixtures.check.ts.
 */

export type Uuid = string;

export const PROTOCOL_VERSION = "0.1.0";

export type Role = "bridge" | "orchestrator";

export interface Hello {
  role: Role;
  bridge_version: string;
  openrct2_version: string;
}

export interface Heartbeat {
  tick: number;
}

/**
 * OpenRCT2's in-game calendar has 8 months per year, not the Gregorian 12,
 * so park_date is this structured triple rather than an ISO calendar date.
 */
export interface ParkDate {
  year: number;
  month: number;
  day: number;
}

export type RideStatus = "open" | "closed" | "testing";

export interface Ride {
  id: number;
  name: string;
  type: string;
  status: RideStatus;
  price: number;
  queue_length: number;
  downtime: number;
}

export interface StaffCounts {
  handyman: number;
  mechanic: number;
  security: number;
  entertainer: number;
}

export type Weather =
  | "sunny"
  | "partially_cloudy"
  | "cloudy"
  | "rain"
  | "heavy_rain"
  | "thunder"
  | "snow"
  | "heavy_snow";

export interface ObservationSnapshot {
  park_date: ParkDate;
  cash: number;
  loan: number;
  park_rating: number;
  guest_count: number;
  rides: Ride[];
  staff_counts: StaffCounts;
  weather: Weather;
}

export type StaffType = "handyman" | "mechanic" | "security" | "entertainer";

/**
 * The five bounded actions the governor may authorize in milestone 0.1 --
 * see docs/PROTOCOL.md and bridge/protocol/commands/*.schema.json. No other
 * action exists in this milestone; adding one requires a schema, a fixture,
 * and a matching Rust variant, not just a new union member here.
 */
export type CommandAction =
  | { action: "set_ride_price"; params: { ride_id: number; price: number } }
  | { action: "set_park_entry_fee"; params: { price: number } }
  | { action: "hire_staff"; params: { type: StaffType } }
  | { action: "open_ride"; params: { ride_id: number } }
  | { action: "close_ride"; params: { ride_id: number } };

export type CommandRequest = CommandAction & {
  idempotency_key: string;
  expiry_tick: number;
};

export interface ErrorInfo {
  code: string;
  message: string;
}

/**
 * command.result payload. The mandatory correlation_id referencing the
 * originating command.request lives on the envelope, not here.
 */
export interface CommandResult {
  engine_cost: number | null;
  engine_error: ErrorInfo | null;
}

export type ShutdownReason = "operator_request" | "fatal_error" | "watchdog_timeout";

export interface Shutdown {
  reason: ShutdownReason;
}

/**
 * ack payload. Empty on purpose: an ack's meaning is entirely carried by the
 * envelope's mandatory correlation_id.
 */
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export type Ack = Record<string, never>;

export type Payload =
  | { kind: "hello"; payload: Hello }
  | { kind: "heartbeat"; payload: Heartbeat }
  | { kind: "observation.snapshot"; payload: ObservationSnapshot }
  | { kind: "command.request"; payload: CommandRequest }
  | { kind: "command.result"; payload: CommandResult }
  | { kind: "shutdown"; payload: Shutdown }
  | { kind: "ack"; payload: Ack };

/**
 * Top-level wire message. See docs/PROTOCOL.md and
 * bridge/protocol/envelope.schema.json for the authoritative shape.
 */
export type Envelope = Payload & {
  protocol_version: "0.1.0";
  message_id: Uuid;
  timestamp: string;
  simulation_id: Uuid;
  correlation_id: Uuid | null;
  status: "ok" | "error" | null;
  error: ErrorInfo | null;
};
