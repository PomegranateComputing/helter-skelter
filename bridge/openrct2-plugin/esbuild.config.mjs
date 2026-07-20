import { build } from "esbuild";

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/plugin.js",
  bundle: true,
  format: "iife",
  target: "es2020",
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
