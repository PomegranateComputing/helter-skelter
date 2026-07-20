import { build } from "esbuild";

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/plugin.js",
  bundle: true,
  format: "iife",
  target: "es2020",
});
