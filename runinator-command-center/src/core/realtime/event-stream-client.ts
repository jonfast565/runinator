import { buildWebSocketUrl } from "./websocket-url";
import type { EventStreamRouter, ServerEvent } from "./event-router";

const RECONNECT_DELAY = 3000;
const FALLBACK_INTERVAL = 30000;
const CONNECT_TIMEOUT = 5000;

export type EventStreamState = "disconnected" | "connecting" | "connected" | "fallback";

export interface EventStreamClientOptions {
  getServiceUrl: () => string | null;
  getServiceKnown: () => boolean;
  onStateChange: (state: EventStreamState) => void;
  onFallbackTick: () => void;
  router: EventStreamRouter;
}

export class EventStreamClient {
  private ws: WebSocket | null = null;
  private fallbackTimer: number | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private connectTimer: ReturnType<typeof setTimeout> | null = null;
  private connectionId = 0;

  constructor(private readonly options: EventStreamClientOptions) {}

  connect() {
    this.clearReconnectTimer();
    this.clearConnectTimer();
    const serviceUrl = this.options.getServiceUrl();

    if (!serviceUrl) {
      this.startFallback();
      return;
    }

    const currentConnection = ++this.connectionId;
    this.options.onStateChange("connecting");
    const url = buildWebSocketUrl(serviceUrl, "/ws/events");
    this.ws = new WebSocket(url);
    this.connectTimer = setTimeout(() => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      this.ws?.close();
      this.startFallback();
    }, CONNECT_TIMEOUT);

    this.ws.onopen = () => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      this.clearConnectTimer();
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
        this.reconnectTimer = setTimeout(() => { this.connect(); }, RECONNECT_DELAY);
      }
    };

    this.ws.onerror = () => {
      if (currentConnection !== this.connectionId) {
        return;
      }

      this.clearConnectTimer();
      this.ws?.close();
    };
  }

  disconnect() {
    this.connectionId += 1;
    this.clearReconnectTimer();
    this.clearConnectTimer();
    this.ws?.close();
    this.ws = null;
    this.stopFallback();
    this.options.onStateChange("disconnected");
  }

  private startFallback() {
    if (this.fallbackTimer !== null) {
      return;
    }

    this.options.onStateChange("fallback");
    this.fallbackTimer = window.setInterval(
      () => { this.options.onFallbackTick(); },
      FALLBACK_INTERVAL,
    );
  }

  private stopFallback() {
    if (this.fallbackTimer !== null) {
      clearInterval(this.fallbackTimer);
      this.fallbackTimer = null;
    }
  }

  private clearReconnectTimer() {
    if (this.reconnectTimer === null) {
      return;
    }

    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private clearConnectTimer() {
    if (this.connectTimer === null) {
      return;
    }

    clearTimeout(this.connectTimer);
    this.connectTimer = null;
  }
}
