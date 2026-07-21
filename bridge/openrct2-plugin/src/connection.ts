/**
 * Owns the TCP connection to the orchestrator: connect, hello, buffered
 * send, reconnect-with-backoff, and inbound NDJSON line framing. Dispatches
 * parsed command.request envelopes to the caller-supplied handler --
 * interpreting *what* a command means (commands.ts) stays out of this
 * module, which only owns the wire.
 */
import type { BridgeConfig } from "./config";
import type { CommandResult, Envelope, Hello, Payload } from "./protocol";
import { PROTOCOL_VERSION } from "./protocol";
import { randomUuidV7 } from "./uuid";

function makeEnvelope(simulationId: string, kindPayload: Payload): Envelope {
  return {
    protocol_version: PROTOCOL_VERSION,
    message_id: randomUuidV7(),
    timestamp: new Date().toISOString(),
    simulation_id: simulationId,
    correlation_id: null,
    status: null,
    error: null,
    ...kindPayload,
  } as Envelope;
}

export type CommandRequestHandler = (envelope: Envelope & { kind: "command.request" }) => void;

export class BridgeConnection {
  private readonly config: BridgeConfig;
  private readonly simulationId: string;
  private readonly onCommandRequest: CommandRequestHandler;
  private socket: Socket | null = null;
  private connected = false;
  private reconnectDelayMs: number;
  private readonly snapshotBuffer: string[] = [];
  private inboundBuffer = "";

  constructor(config: BridgeConfig, simulationId: string, onCommandRequest: CommandRequestHandler) {
    this.config = config;
    this.simulationId = simulationId;
    this.onCommandRequest = onCommandRequest;
    this.reconnectDelayMs = config.initialReconnectDelayMs;
  }

  start(): void {
    this.connect();
  }

  sendHeartbeat(tick: number): void {
    if (!this.connected) {
      // Stale heartbeats are meaningless; only sent while actually connected.
      return;
    }
    this.write(makeEnvelope(this.simulationId, { kind: "heartbeat", payload: { tick } }));
  }

  sendSnapshot(payload: Payload & { kind: "observation.snapshot" }): void {
    const envelope = makeEnvelope(this.simulationId, payload);
    const line = JSON.stringify(envelope);
    if (this.connected && this.socket) {
      this.writeLine(line);
    } else {
      this.snapshotBuffer.push(line);
      while (this.snapshotBuffer.length > this.config.maxBufferedSnapshots) {
        this.snapshotBuffer.shift();
      }
    }
  }

  /** Sends a command.result correlated back to the command.request that triggered it. */
  sendCommandResult(correlationId: string, result: CommandResult): void {
    const envelope = makeEnvelope(this.simulationId, { kind: "command.result", payload: result });
    envelope.correlation_id = correlationId;
    envelope.status = result.engine_error ? "error" : "ok";
    this.write(envelope);
  }

  private connect(): void {
    try {
      this.socket = network.createSocket();
      this.socket.on("data", (data) => this.onData(data));
      this.socket.on("close", () => this.onClose());
      this.socket.on("error", (errorString) => this.onError(errorString));
      this.socket.connect(this.config.port, this.config.host, () => this.onConnect());
    } catch (err) {
      console.log(`[bridge] connect() threw: ${String(err)}`);
      this.scheduleReconnect();
    }
  }

  private onConnect(): void {
    console.log(`[bridge] connected to ${this.config.host}:${this.config.port}`);
    this.connected = true;
    this.reconnectDelayMs = this.config.initialReconnectDelayMs;

    const hello: Hello = {
      role: "bridge",
      bridge_version: "0.1.0",
      openrct2_version: String(context.apiVersion),
    };
    this.write(makeEnvelope(this.simulationId, { kind: "hello", payload: hello }));
    this.flushSnapshotBuffer();
  }

  private onData(data: string): void {
    // TCP delivers a byte stream, not necessarily one "data" event per
    // NDJSON line -- buffer and split on "\n", keeping any incomplete
    // trailing fragment for the next event.
    this.inboundBuffer += data;
    let newlineIndex = this.inboundBuffer.indexOf("\n");
    while (newlineIndex !== -1) {
      const line = this.inboundBuffer.slice(0, newlineIndex);
      this.inboundBuffer = this.inboundBuffer.slice(newlineIndex + 1);
      if (line.trim().length > 0) {
        this.handleLine(line);
      }
      newlineIndex = this.inboundBuffer.indexOf("\n");
    }
  }

  private handleLine(line: string): void {
    let envelope: Envelope;
    try {
      envelope = JSON.parse(line) as Envelope;
    } catch (err) {
      console.log(`[bridge] received unparseable line: ${String(err)}`);
      return;
    }

    if (envelope.kind === "command.request") {
      this.onCommandRequest(envelope as Envelope & { kind: "command.request" });
    } else {
      // shutdown/ack: not yet acted on in this milestone. Logged so a
      // human can see traffic during development.
      console.log(`[bridge] received: ${line}`);
    }
  }

  private onClose(): void {
    if (this.connected) {
      console.log("[bridge] connection closed");
    }
    this.connected = false;
    this.socket = null;
    this.scheduleReconnect();
  }

  private onError(errorString: string): void {
    console.log(`[bridge] socket error: ${errorString}`);
    this.connected = false;
    this.socket = null;
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    const delay = this.reconnectDelayMs;
    this.reconnectDelayMs = Math.min(this.reconnectDelayMs * 2, this.config.maxReconnectDelayMs);
    context.setTimeout(() => this.connect(), delay);
  }

  private flushSnapshotBuffer(): void {
    while (this.snapshotBuffer.length > 0) {
      const line = this.snapshotBuffer.shift();
      if (line !== undefined) {
        this.writeLine(line);
      }
    }
  }

  private write(envelope: Envelope): void {
    this.writeLine(JSON.stringify(envelope));
  }

  private writeLine(line: string): void {
    if (!this.socket) {
      return;
    }
    try {
      this.socket.write(`${line}\n`);
    } catch (err) {
      console.log(`[bridge] write() threw: ${String(err)}`);
    }
  }
}
