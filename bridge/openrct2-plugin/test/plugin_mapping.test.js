// Unit tests for the plugin's pure engine-value -> protocol-value mapping
// functions, plus the UUIDv7 generator's wire-format compliance. These are
// the only parts of the plugin testable without a running OpenRCT2
// process; buildObservationSnapshot() itself reads live game globals
// (park/map/climate/date) and is exercised instead by the real headless
// run captured in docs/OPENRCT2_INTEGRATION.md / the PR description.
import { test } from "node:test";
import assert from "node:assert/strict";

import { mapWeather, mapRideStatus, randomUuidV7 } from "../dist/test-exports.js";

const UUIDV7_PATTERN =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-7[0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$/;

test("mapWeather covers every real climate.current.weather value", () => {
  // The full set from doc/openrct2.d.ts's WeatherType union -- see
  // docs/OPENRCT2_INTEGRATION.md.
  const cases = {
    sunny: "sunny",
    partiallyCloudy: "partially_cloudy",
    cloudy: "cloudy",
    rain: "rain",
    heavyRain: "heavy_rain",
    thunder: "thunder",
    snow: "snow",
    heavySnow: "heavy_snow",
    blizzard: "heavy_snow", // no protocol slot for blizzard -- documented gap
  };
  for (const [input, expected] of Object.entries(cases)) {
    assert.equal(mapWeather(input), expected, `mapWeather(${input})`);
  }
});

test("mapRideStatus covers every real Ride.status value", () => {
  assert.equal(mapRideStatus("open"), "open");
  assert.equal(mapRideStatus("closed"), "closed");
  assert.equal(mapRideStatus("testing"), "testing");
  // "simulating" has no protocol slot (documented gap) -- mapped to the
  // closest existing value rather than throwing.
  assert.equal(mapRideStatus("simulating"), "testing");
});

test("randomUuidV7 matches the envelope schema's uuidv7 pattern", () => {
  for (let i = 0; i < 20; i++) {
    const id = randomUuidV7();
    assert.match(id, UUIDV7_PATTERN, `randomUuidV7() produced ${id}`);
  }
});

test("randomUuidV7 is time-ordered", () => {
  const first = randomUuidV7();
  const second = randomUuidV7();
  assert.ok(first < second || first === second, "successive UUIDv7s should sort non-decreasing");
});
