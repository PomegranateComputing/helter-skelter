/**
 * Owns the TCP connection to the orchestrator: connect, hello, buffered
 * send, and reconnect-with-backoff. No business logic and no parsing of
 * inbound data beyond logging it -- this milestone is observe/transmit
 * only (core/orchestrator is the one making decisions).
 */
import type { BridgeConfig } from "./config";
import type { Envelope, Hello, Payload } from "./protocol";
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

export class BridgeConnection {
  private readonly config: BridgeConfig;
  private readonly simulationId: string;
  private socket: Socket | null = null;
  private connected = false;
  private reconnectDelayMs: number;
  private readonly snapshotBuffer: string[] = [];

  constructor(config: BridgeConfig, simulationId: string) {
    this.config = config;
    this.simulationId = simulationId;
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
    // Observe/transmit only in this milestone -- inbound messages (acks,
    // command.request from the orchestrator) are not yet acted on. Logged
    // so a human can see traffic during development.
    console.log(`[bridge] received: ${data}`);
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
