/**
 * The plugin scripting sandbox has no filesystem API (confirmed: no
 * fs/readFile binding anywhere in doc/openrct2.d.ts -- see
 * docs/OPENRCT2_INTEGRATION.md), so config/bridge.json cannot be read at
 * plugin runtime. Instead esbuild.config.mjs reads it at *build* time (a
 * normal Node.js process, which does have fs access) and inlines it via
 * `define`, replacing the identifier below with the JSON literal. This is
 * a build-time configuration mechanism, not a runtime one.
 */
export interface BridgeConfig {
  host: string;
  port: number;
  heartbeatIntervalTicks: number;
  snapshotIntervalTicks: number;
  maxBufferedSnapshots: number;
  initialReconnectDelayMs: number;
  maxReconnectDelayMs: number;
}

declare const __BRIDGE_CONFIG__: BridgeConfig;

export const bridgeConfig: BridgeConfig = __BRIDGE_CONFIG__;
