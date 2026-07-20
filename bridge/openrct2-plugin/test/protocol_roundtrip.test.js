// Proves that bridge/protocol/*.schema.json, the fixtures in
// bridge/messages/fixtures/, and this package's TS protocol module all
// agree -- the JS-side mirror of core/common/tests/protocol_roundtrip.rs.
//
// For every fixture:
// 1. It must validate against envelope.schema.json.
// 2. Its `payload` must validate against the schema matching its `kind`.
// 3. For `command.request`, `payload.params` must validate against the
//    schema matching its `action`.
// 4. It must round-trip losslessly through decodeEnvelope/encodeEnvelope.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { Ajv2020 } from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

import { decodeEnvelope, encodeEnvelope } from "../dist/protocol.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const protocolDir = path.join(here, "..", "..", "protocol");
const fixturesDir = path.join(here, "..", "..", "messages", "fixtures");

const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);

function loadSchema(relativePath) {
  const text = readFileSync(path.join(protocolDir, relativePath), "utf8");
  return JSON.parse(text);
}

const validatorCache = new Map();
function validatorFor(relativePath) {
  if (!validatorCache.has(relativePath)) {
    validatorCache.set(relativePath, ajv.compile(loadSchema(relativePath)));
  }
  return validatorCache.get(relativePath);
}

function messageSchemaPath(kind) {
  switch (kind) {
    case "hello":
      return "messages/hello.schema.json";
    case "heartbeat":
      return "messages/heartbeat.schema.json";
    case "observation.snapshot":
      return "messages/observation_snapshot.schema.json";
    case "command.request":
      return "messages/command_request.schema.json";
    case "command.result":
      return "messages/command_result.schema.json";
    case "shutdown":
      return "messages/shutdown.schema.json";
    case "ack":
      return "messages/ack.schema.json";
    default:
      throw new Error(`unknown message kind in fixture: ${kind}`);
  }
}

function commandSchemaPath(action) {
  return `commands/${action}.schema.json`;
}

function loadFixtures() {
  return readdirSync(fixturesDir)
    .filter((name) => name.endsWith(".json"))
    .sort()
    .map((name) => [name, JSON.parse(readFileSync(path.join(fixturesDir, name), "utf8"))]);
}

test("every fixture validates against its schema and round-trips", () => {
  const envelopeValidator = validatorFor("envelope.schema.json");

  for (const [name, fixture] of loadFixtures()) {
    const envelopeOk = envelopeValidator(fixture);
    assert.ok(envelopeOk, `${name}: fails envelope.schema.json: ${ajv.errorsText(envelopeValidator.errors)}`);

    const messageValidator = validatorFor(messageSchemaPath(fixture.kind));
    const payloadOk = messageValidator(fixture.payload);
    assert.ok(
      payloadOk,
      `${name}: payload fails ${messageSchemaPath(fixture.kind)}: ${ajv.errorsText(messageValidator.errors)}`,
    );

    if (fixture.kind === "command.request") {
      const commandValidator = validatorFor(commandSchemaPath(fixture.payload.action));
      const paramsOk = commandValidator(fixture.payload.params);
      assert.ok(
        paramsOk,
        `${name}: params fail ${commandSchemaPath(fixture.payload.action)}: ${ajv.errorsText(commandValidator.errors)}`,
      );
    }

    const decoded = decodeEnvelope(JSON.stringify(fixture));
    const reencoded = JSON.parse(encodeEnvelope(decoded));
    assert.deepStrictEqual(reencoded, fixture, `${name}: round-trip through decodeEnvelope/encodeEnvelope changed shape`);
  }
});

test("command.result and ack fixtures carry a non-null correlation_id", () => {
  for (const [name, fixture] of loadFixtures()) {
    if (fixture.kind === "command.result" || fixture.kind === "ack") {
      assert.notEqual(fixture.correlation_id, null, `${name}: ${fixture.kind} must carry a correlation_id`);
    }
  }
});
