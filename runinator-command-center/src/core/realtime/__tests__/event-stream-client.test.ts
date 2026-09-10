import { describe, it, expect, vi } from "vitest";
import {
  EventStreamClient,
  type EventSocket,
  type EventStreamTimers,
} from "../event-stream-client";

function harness() {
  let id = 0;
  const pending = new Map<number, { callback: () => void; delay: number; interval: boolean }>();
  const timers: EventStreamTimers = {
    setTimeout(callback, delay) {
      pending.set(++id, { callback, delay, interval: false });
      return id;
    },
    clearTimeout(handle) {
      pending.delete(handle);
    },
    setInterval(callback, delay) {
      pending.set(++id, { callback, delay, interval: true });
      return id;
    },
    clearInterval(handle) {
      pending.delete(handle);
    },
  };
  const sockets: EventSocket[] = [];
  const onStateChange = vi.fn();
  const onFallbackTick = vi.fn();
  const route = vi.fn();
  const client = new EventStreamClient({
    getServiceUrl: () => "http://localhost:8080",
    getServiceKnown: () => true,
    onStateChange,
    onFallbackTick,
    router: { route },
    timers,
    sockets: {
      create() {
        const socket: EventSocket = {
          readyState: 0,
          onopen: null,
          onclose: null,
          onerror: null,
          onmessage: null,
          close: vi.fn(),
        };
        sockets.push(socket);
        return socket;
      },
    },
  });

  function fire(delay: number) {
    for (const [handle, task] of [...pending]) {
      if (task.delay !== delay) {
        continue;
      }

      if (!task.interval) {
        pending.delete(handle);
      }

      task.callback();
    }
  }

  function open(socket: EventSocket) {
    socket.onopen?.call(socket as WebSocket, new Event("open"));
  }

  function close(socket: EventSocket) {
    socket.onclose?.call(socket as WebSocket, new CloseEvent("close"));
  }

  return { client, sockets, pending, fire, open, close, onStateChange, onFallbackTick, route };
}

describe("injected event stream transports", () => {
  it("keeps the deadline after a connecting error", () => {
    const h = harness();
    h.client.connect();
    const socket = h.sockets[0];
    socket.onerror?.call(socket as WebSocket, new Event("error"));
    h.fire(5000);
    expect(h.onStateChange).toHaveBeenLastCalledWith("fallback");
  });
  it("retires an open socket when explicitly reconnected", () => {
    const h = harness();
    h.client.connect();
    const socket = h.sockets[0];
    Object.assign(socket, { readyState: 1 });
    h.open(socket);
    h.client.connect();
    expect(socket.close).toHaveBeenCalledOnce();
    h.close(socket);
    expect(h.onStateChange).toHaveBeenLastCalledWith("connecting");
  });
  it("closes a late upgrade after fallback without changing state", () => {
    const h = harness();
    h.client.connect();
    h.fire(5000);
    h.open(h.sockets[0]);
    expect(h.sockets[0].close).toHaveBeenCalledOnce();
    expect(h.onStateChange).toHaveBeenLastCalledWith("fallback");
    h.fire(30000);
    expect(h.onFallbackTick).toHaveBeenCalledOnce();
    h.client.disconnect();
    expect(h.pending.size).toBe(0);
  });
  it("invalidates callbacks and cancels reconnect on disconnect", () => {
    const h = harness();
    h.client.connect();
    h.open(h.sockets[0]);
    h.close(h.sockets[0]);
    expect(h.pending.size).toBe(2);
    h.client.disconnect();
    h.open(h.sockets[0]);
    h.close(h.sockets[0]);
    expect(h.pending.size).toBe(0);
    expect(h.onStateChange).toHaveBeenLastCalledWith("disconnected");
  });
  it("reconnects and stops fallback after opening the replacement", () => {
    const h = harness();
    h.client.connect();
    h.open(h.sockets[0]);
    h.close(h.sockets[0]);
    const retry = [...h.pending.values()].find((task) => !task.interval)!;
    h.fire(retry.delay);
    expect(h.sockets).toHaveLength(2);
    h.open(h.sockets[1]);
    expect(h.pending.size).toBe(0);
    expect(h.onStateChange).toHaveBeenLastCalledWith("connected");
  });
});
