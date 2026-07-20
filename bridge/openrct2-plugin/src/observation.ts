/**
 * Builds an observation.snapshot payload from the live game state, using
 * only API calls verified in docs/OPENRCT2_INTEGRATION.md. No business
 * logic here -- this module only reads and reshapes state.
 */
import type { ObservationSnapshot, Ride as ProtocolRide, StaffCounts, Weather } from "./protocol";

// date.month is 0-indexed (0 = March .. 7 = October) per doc/openrct2.d.ts;
// our protocol's park_date.month is 1-indexed (1-8). See docs/OPENRCT2_INTEGRATION.md.
function buildParkDate() {
  return { year: date.year, month: date.month + 1, day: date.day };
}

// climate.current.weather uses camelCase ("partiallyCloudy", "heavyRain",
// "heavySnow") and includes "blizzard", which our protocol schema doesn't
// (yet) have a slot for -- a real discrepancy, documented in
// docs/OPENRCT2_INTEGRATION.md. "blizzard" maps to "heavy_snow" here as the
// closest existing value rather than dropping the field or crashing.
export function mapWeather(raw: string): Weather {
  switch (raw) {
    case "sunny":
      return "sunny";
    case "partiallyCloudy":
      return "partially_cloudy";
    case "cloudy":
      return "cloudy";
    case "rain":
      return "rain";
    case "heavyRain":
      return "heavy_rain";
    case "thunder":
      return "thunder";
    case "snow":
      return "snow";
    case "heavySnow":
    case "blizzard":
      return "heavy_snow";
    default:
      return "sunny";
  }
}

// Ride.status is already the string union "closed"|"open"|"testing"|"simulating"
// when *reading* (unlike the numeric status used when *writing* via the
// ridesetstatus game action). Our protocol's ride.status enum is missing
// "simulating" (documented gap) -- mapped to "testing" here as the closest
// existing value.
export function mapRideStatus(raw: string): ProtocolRide["status"] {
  if (raw === "open" || raw === "closed" || raw === "testing") {
    return raw;
  }
  return "testing";
}

function buildRides(): ProtocolRide[] {
  return map.rides.map((ride) => ({
    id: ride.id,
    name: ride.name,
    // ride.object.identifier is a stable string like "rct2.wmouse", unlike
    // the numeric ride.type (internal built-in ride type ID).
    type: ride.object.identifier,
    status: mapRideStatus(ride.status),
    // price is [primary, secondary?] -- see docs/OPENRCT2_INTEGRATION.md.
    price: ride.price[0] ?? 0,
    // GAP: the scripting API exposes no ride queue length anywhere (neither
    // on Ride/RideStation nor derivable from Peep/Guest state) -- see
    // docs/OPENRCT2_INTEGRATION.md. 0 is a placeholder, not a measurement.
    queue_length: 0,
    downtime: ride.downtime,
  }));
}

function buildStaffCounts(): StaffCounts {
  const counts: StaffCounts = { handyman: 0, mechanic: 0, security: 0, entertainer: 0 };
  for (const staff of map.getAllEntities("staff")) {
    if (staff.staffType in counts) {
      counts[staff.staffType as keyof StaffCounts] += 1;
    }
  }
  return counts;
}

export function buildObservationSnapshot(): ObservationSnapshot {
  return {
    park_date: buildParkDate(),
    cash: park.cash,
    loan: park.bankLoan,
    park_rating: park.rating,
    guest_count: park.guests,
    rides: buildRides(),
    staff_counts: buildStaffCounts(),
    weather: mapWeather(climate.current.weather),
  };
}
