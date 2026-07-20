#!/usr/bin/env node
// Stand-in for core/orchestrator: listens on the bridge's configured port,
// accepts one connection, and prints every newline-delimited JSON message
// it receives. For manually watching the bridge plugin's real traffic
// end-to-end -- it does not reply, authorize, or persist anything.
//
// Usage: node scripts/dev/fake-orchestrator.js [--count N]
//   --count N   exit after printing N messages (default: run until Ctrl-C)

import { createServer } from "node:net";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.join(here, "..", "..");
const bridgeConfig = JSON.parse(readFileSync(path.join(repoRoot, "config", "bridge.json"), "utf8"));

const countFlagIndex = process.argv.indexOf("--count");
const maxMessages = countFlagIndex !== -1 ? Number(process.argv[countFlagIndex + 1]) : Infinity;

let messageCount = 0;

function log(...args) {
  console.log(`[fake-orchestrator]`, ...args);
}

const server = createServer((socket) => {
  log(`bridge connected from ${socket.remoteAddress}:${socket.remotePort}`);

  let buffer = "";
  socket.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    for (let newlineIndex = buffer.indexOf("\n"); newlineIndex !== -1; newlineIndex = buffer.indexOf("\n")) {
      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      if (line.length === 0) {
        continue;
      }

      messageCount += 1;
      try {
        const envelope = JSON.parse(line);
        log(
          `#${messageCount} kind=${envelope.kind} message_id=${envelope.message_id} timestamp=${envelope.timestamp}`,
        );
        log(JSON.stringify(envelope, null, 2));
      } catch (err) {
        log(`#${messageCount} (unparseable line): ${line}`, err);
      }

      if (messageCount >= maxMessages) {
        log(`received ${maxMessages} message(s), exiting.`);
        socket.end();
        server.close();
        process.exit(0);
      }
    }
  });

  socket.on("close", () => log("bridge disconnected"));
  socket.on("error", (err) => log("socket error:", err.message));
});

server.listen(bridgeConfig.port, bridgeConfig.host, () => {
  log(`listening on ${bridgeConfig.host}:${bridgeConfig.port}`);
});
