import { buildWebSocketProtocols, buildWebSocketUrl } from "./websocket-url";
import type { EventStreamRouter, ServerEvent } from "./event-router";
import { ReconnectBackoff } from "./reconnect-backoff";

const FALLBACK_INTERVAL = 30000;
const CONNECT_TIMEOUT = 5000;
const SOCKET_OPEN = 1;

export type EventStreamState = "disconnected" | "connecting" | "connected" | "fallback";

export type EventSocket = Pick<
  WebSocket,
  "onopen" | "onmessage" | "onclose" | "onerror" | "readyState" | "close"
>;
export interface WebSocketFactory {
  create(url: string, protocols: string[]): EventSocket;
}
export interface EventStreamTimers {
  setTimeout(callback: () => void, delay: number): number;
  clearTimeout(handle: number): void;
  setInterval(callback: () => void, delay: number): number;
  clearInterval(handle: number): void;
}
const browserSockets: WebSocketFactory = {
  create: (url, protocols) => new WebSocket(url, protocols),
};
const browserTimers: EventStreamTimers = {
  setTimeout: (callback, delay) => window.setTimeout(callback, delay),
  clearTimeout: (handle) => {
    window.clearTimeout(handle);
  },
  setInterval: (callback, delay) => window.setInterval(callback, delay),
  clearInterval: (handle) => {
    window.clearInterval(handle);
  },
};

export interface EventStreamClientOptions {
  getServiceUrl: () => string | null;
  getServiceKnown: () => boolean;
  onStateChange: (state: EventStreamState) => void;
  onFallbackTick: () => void;
  router: EventStreamRouter;
  sockets?: WebSocketFactory;
  timers?: EventStreamTimers;
}

export class EventStreamClient {
  private ws: EventSocket | null = null;
  private fallbackTimer: number | null = null;
  private reconnectTimer: number | null = null;
  private connectTimer: number | null = null;
  private connectionId = 0;
  private readonly backoff = new ReconnectBackoff();

  private readonly sockets: WebSocketFactory;
  private readonly timers: EventStreamTimers;
  constructor(private readonly options: EventStreamClientOptions) {
    this.sockets = options.sockets ?? browserSockets;
    this.timers = options.timers ?? browserTimers;
  }

  connect() {
    this.clearReconnectTimer();
    this.clearConnectTimer();
    const currentConnection = ++this.connectionId;

    if (this.ws?.readyState === SOCKET_OPEN) {
      this.ws.close();
    }

    this.ws = null;
    const serviceUrl = this.options.getServiceUrl();

    if (!serviceUrl) {
      this.startFallback();
      return;
    }

    this.options.onStateChange("connecting");
    const url = buildWebSocketUrl(serviceUrl, "/ws/events");
    const socket = this.sockets.create(url, buildWebSocketProtocols());
    this.ws = socket;
    this.connectTimer = this.timers.setTimeout(() => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      // `close()` during CONNECTING produces a noisy browser error. Invalidate this attempt and
      // let a late upgrade close itself in `onopen`, where closing is intentional and quiet.
      this.connectionId += 1;

      if (this.ws === socket) {
        this.ws = null;
      }

      this.startFallback();
    }, CONNECT_TIMEOUT);

    this.ws.onopen = () => {
      if (currentConnection !== this.connectionId) {
        socket.close();
        return;
      }

      this.clearConnectTimer();
      this.backoff.reset();
      this.options.onStateChange("connected");
      this.stopFallback();
    };

    this.ws.onmessage = ({ data }: MessageEvent<string>) => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      try {
        this.options.router.route(JSON.parse(data) as ServerEvent);
      } catch {
        /* ignore malformed payloads */
      }
    };

    this.ws.onclose = () => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      this.clearConnectTimer();
      this.ws = null;
      this.startFallback();

      if (this.options.getServiceKnown()) {
        this.reconnectTimer = this.timers.setTimeout(() => {
          this.connect();
        }, this.backoff.next());
      }
    };

    this.ws.onerror = () => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      if (socket.readyState === SOCKET_OPEN) {
        this.clearConnectTimer();
        socket.close();
      }
    };
  }

  disconnect() {
    this.connectionId += 1;
    this.clearReconnectTimer();
    this.clearConnectTimer();
    this.backoff.reset();

    if (this.ws?.readyState === SOCKET_OPEN) {
      this.ws.close();
    }

    this.ws = null;
    this.stopFallback();
    this.options.onStateChange("disconnected");
  }

  private startFallback() {
    if (this.fallbackTimer !== null) {
      return;
    }

    this.options.onStateChange("fallback");
    this.fallbackTimer = this.timers.setInterval(() => {
      this.options.onFallbackTick();
    }, FALLBACK_INTERVAL);
  }

  private stopFallback() {
    if (this.fallbackTimer !== null) {
      this.timers.clearInterval(this.fallbackTimer);
      this.fallbackTimer = null;
    }
  }

  private clearReconnectTimer() {
    if (this.reconnectTimer === null) {
      return;
    }

    this.timers.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private clearConnectTimer() {
    if (this.connectTimer === null) {
      return;
    }

    this.timers.clearTimeout(this.connectTimer);
    this.connectTimer = null;
  }
}
