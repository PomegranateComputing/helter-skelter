import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";

test("plugin bundle builds", () => {
  assert.ok(existsSync(new URL("../dist/plugin.js", import.meta.url)));
});
