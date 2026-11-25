// The transport seam. Every shell implements `Bridge`; the components only
// ever see this interface, so the same UI runs under Tauri, in a browser, and
// inside the plugin.

import { isResponse, type Request, type ServerMessage } from "./protocol";

export interface Bridge {
  /** Send a command and await its response. */
  request(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  /** Subscribe to server-initiated messages. Returns an unsubscribe fn. */
  subscribe(cb: (msg: ServerMessage) => void): () => void;
  /** Close the connection. */
  close(): void;
}

/**
 * WebSocket transport. Used by the Tauri shell and by `npm run dev`.
 *
 * The URL resolution order is deliberate:
 *   1. an explicit constructor argument (tests, unusual shells)
 *   2. `/engine` — the Vite dev proxy, so dev shares an origin with the app
 *   3. the build-time default (loopback, per the protocol)
 */
export class WebSocketBridge implements Bridge {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private listeners = new Set<(msg: ServerMessage) => void>();
  private closed = false;

  constructor(private url: string = defaultUrl()) {
    this.connect();
  }

  private connect() {
    const ws = new WebSocket(this.url);
    console.error("[bridge] connecting to", this.url, "readyState", ws.readyState);
    this.ws = ws;
    ws.onmessage = (ev) => this.onMessage(String(ev.data));
    ws.onclose = () => {
      if (!this.closed) {
        for (const { reject } of this.pending.values()) reject(new Error("engine disconnected"));
        this.pending.clear();
        console.error("[bridge] ws closed, reconnecting");
        setTimeout(() => this.connect(), 1000);
      }
    };
    ws.onerror = () => {
      console.error("[bridge] ws error, closing");
      ws.close();
    };
  }

  private onMessage(text: string) {
    let msg: ServerMessage;
    try {
      msg = JSON.parse(text) as ServerMessage;
    } catch {
      return;
    }
    if (isResponse(msg)) {
      const entry = this.pending.get(msg.id);
      if (!entry) return;
      this.pending.delete(msg.id);
      if (msg.ok) {
        entry.resolve(msg.payload);
      } else {
        const e = new Error(msg.error ?? "request failed");
        entry.reject(e);
        // eslint-disable-next-line no-console
        console.error("[bridge] rejected", msg.id, msg.error);
      }
      return;
    }
    for (const cb of this.listeners) cb(msg);
  }

  request(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
    const id = this.nextId++;
    const req: Request = { type: "req", id, cmd, args };
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        // Queue until the socket is up; the engine greets with a snapshot, so
        // a request sent in the first moments is still safe.
        const timer = setInterval(() => {
          if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            clearInterval(timer);
            this.ws.send(JSON.stringify(req));
          }
        }, 20);
        setTimeout(() => {
          clearInterval(timer);
          if (this.pending.has(id)) {
            this.pending.delete(id);
            reject(new Error("engine unreachable"));
          }
        }, 5000);
        return;
      }
      this.ws.send(JSON.stringify(req));
    });
  }

  subscribe(cb: (msg: ServerMessage) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  close() {
    this.closed = true;
    this.ws?.close();
    for (const { reject } of this.pending.values()) reject(new Error("closed"));
    this.pending.clear();
  }
}

/** The in-plugin bridge. The native shell injects this object. */
export interface NativeBridge {
  pedalRequest(cmd: string, args?: Record<string, unknown>): Promise<string>;
  pedalSubscribe(cb: (json: string) => void): () => void;
}

declare global {
  interface Window {
    __pedal_native?: NativeBridge;
  }
}

/**
 * Bridges to the plugin's native shell. Requests return JSON strings; events
 * arrive as JSON strings too, so the parsing lives in one place.
 */
export class PluginBridge implements Bridge {
  constructor(private native: NativeBridge = window.__pedal_native!) {}

  request(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
    return this.native.pedalRequest(cmd, args).then((s) => JSON.parse(s));
  }

  subscribe(cb: (msg: ServerMessage) => void): () => void {
    return this.native.pedalSubscribe((json) => {
      try {
        cb(JSON.parse(json) as ServerMessage);
      } catch {
        /* ignore malformed events */
      }
    });
  }

  close() {}
}

/** Pick the right bridge for this environment. */
export function createBridge(): Bridge {
  if (typeof window !== "undefined" && window.__pedal_native) {
    return new PluginBridge();
  }
  return new WebSocketBridge();
}

declare global {
  interface Window {
    /** Injected by the shell: the loopback URL of *this* shell's own engine. */
    __PEDAL_ENGINE_URL__?: string;
  }
}

function defaultUrl(): string {
  if (typeof window !== "undefined" && window.__PEDAL_ENGINE_URL__) return window.__PEDAL_ENGINE_URL__;
  const injected = (import.meta as unknown as { env?: Record<string, string> }).env
    ?.VITE_ENGINE_URL;
  if (injected) return injected;
  // Vite replaces this at build time; see vite.config.ts.
  const buildTime = "__ENGINE_URL__";
  if (buildTime !== "__ENGINE_URL__") return buildTime;
  // Dev server: same origin, proxied by Vite.
  if (import.meta.env?.DEV) return `${location.origin.replace(/^http/, "ws")}/engine`;
  throw new Error("no engine URL: the shell injects window.__PEDAL_ENGINE_URL__ (see docs/RUN-THE-APP.md)");
}
