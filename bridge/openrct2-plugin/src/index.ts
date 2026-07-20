import { bridgeConfig } from "./config";
import { BridgeConnection } from "./connection";
import { buildObservationSnapshot } from "./observation";
import { randomUuidV7 } from "./uuid";

registerPlugin({
  name: "helter-skelter-bridge",
  version: "0.1.0",
  authors: ["Helter Skelter"],
  // "intransient" (not "remote"): this plugin must stay loaded across park
  // changes rather than unloading with the scenario -- see
  // docs/OPENRCT2_INTEGRATION.md's "Plugin type semantics" section.
  type: "intransient",
  licence: "MIT",
  targetApiVersion: 115,
  main() {
    const simulationId = randomUuidV7();
    const connection = new BridgeConnection(bridgeConfig, simulationId);
    connection.start();

    context.subscribe("interval.tick", () => {
      // Never let an observation/transmission failure crash the game --
      // this plugin is a thin observer, not the decision-maker.
      try {
        const tick = date.ticksElapsed;

        if (tick % bridgeConfig.heartbeatIntervalTicks === 0) {
          connection.sendHeartbeat(tick);
        }

        if (tick % bridgeConfig.snapshotIntervalTicks === 0) {
          connection.sendSnapshot({ kind: "observation.snapshot", payload: buildObservationSnapshot() });
        }
      } catch (err) {
        console.log(`[bridge] tick handler error: ${String(err)}`);
      }
    });
  },
});
