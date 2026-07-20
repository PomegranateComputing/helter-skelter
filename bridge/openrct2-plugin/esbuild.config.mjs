import { readFileSync } from "node:fs";
import { build } from "esbuild";

// config/bridge.json is inlined at build time -- the plugin sandbox has no
// filesystem API to read it at runtime. See src/config.ts.
const bridgeConfig = JSON.parse(readFileSync("../../config/bridge.json", "utf8"));

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/plugin.js",
  bundle: true,
  format: "iife",
  target: "es2020",
  define: {
    __BRIDGE_CONFIG__: JSON.stringify(bridgeConfig),
  },
});

// Bundled separately (ESM, not IIFE) so tests can import the protocol
// module's typed encode/decode helpers directly.
await build({
  entryPoints: ["src/protocol/index.ts"],
  outfile: "dist/protocol.js",
  bundle: true,
  format: "esm",
  target: "es2020",
});

// Pure functions with no game-global dependency, bundled separately so
// tests can exercise them without a running OpenRCT2 process. Named
// pure.ts/pure.js, not test*.ts -- `node --test` with no path arguments
// discovers files by a "test" substring in the name and will try (and
// fail) to execute a matching *source* .ts file directly.
await build({
  entryPoints: ["src/pure.ts"],
  outfile: "dist/pure.js",
  bundle: true,
  format: "esm",
  target: "es2020",
});
