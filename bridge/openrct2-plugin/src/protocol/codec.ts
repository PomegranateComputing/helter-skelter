/**
 * Typed (de)serialization only. Whether a message is well-formed (schema
 * shape, correlation_id-mandatory-for-some-kinds, etc.) is the concern of
 * core/governor and the fixture-driven tests in this package and
 * core/common, not of the plugin at runtime -- the bridge stays a thin
 * observer/translator per docs/VISION.md.
 */
import type { Envelope } from "./types";

export function encodeEnvelope(envelope: Envelope): string {
  return JSON.stringify(envelope);
}

export function decodeEnvelope(json: string): Envelope {
  return JSON.parse(json) as Envelope;
}
