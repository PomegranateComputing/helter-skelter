// Demo/test-seeding plugin -- NOT part of the committed bridge
// (bridge/openrct2-plugin/). The dev park's rides start at price 0, and
// the bridge's queue_length is a hardcoded 0 placeholder (see
// docs/OPENRCT2_INTEGRATION.md's GAPS section), so neither operator rule
// has room to fire against the park's real starting state. This plugin
// runs once at park load and bumps every ride's price up so the
// decrease rule can trigger for real -- used by
// scripts/dev/chaos/kill-orchestrator-with-action-in-flight.sh to force
// a real action deterministically. Same approach used for the phase 6/7
// real end-to-end proof runs; kept here (unlike those) because chaos
// tests need to be repeatable, not one-off.
registerPlugin({
    name: "helter-skelter-chaos-seed",
    version: "0.0.0",
    authors: ["Helter Skelter"],
    type: "local",
    licence: "MIT",
    targetApiVersion: 68,
    main() {
        const rides = map.rides.filter((r) => r.classification === "ride");
        for (const ride of rides) {
            context.executeAction("ridesetprice", { ride: ride.id, price: 5, isPrimaryPrice: true }, (result) => {
                console.log(`[seed] ride ${ride.id} (${ride.name}) price -> 5, result: ${JSON.stringify(result)}`);
            });
        }
    },
});
