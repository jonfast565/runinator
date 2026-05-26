import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { effectScope, nextTick } from "vue";
import { useEventStream } from "../useEventStream";
import { useAppStore } from "../../stores/app";

class MockWebSocket {
  static sockets: MockWebSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  close = vi.fn(() => {
    this.onclose?.();
  });

  constructor(public readonly url: string) {
    MockWebSocket.sockets.push(this);
  }
}

describe("useEventStream", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setActivePinia(createPinia());
    MockWebSocket.sockets = [];
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal("window", {
      clearInterval,
      clearTimeout,
      setInterval,
      setTimeout
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("falls back when the event stream connect attempt times out", async () => {
    const app = useAppStore();
    app.setServiceUrl("http://127.0.0.1:8080/");

    const scope = effectScope();
    scope.run(() => useEventStream());
    await nextTick();

    expect(app.eventStreamState).toBe("connecting");
    expect(MockWebSocket.sockets[0].url).toBe("ws://127.0.0.1:8080/ws/events");

    vi.advanceTimersByTime(5000);

    expect(app.eventStreamState).toBe("fallback");
    expect(MockWebSocket.sockets[0].close).toHaveBeenCalled();
    scope.stop();
  });

  it("ignores stale close events from an old connection", async () => {
    const app = useAppStore();
    app.setServiceUrl("http://127.0.0.1:8080/");

    const scope = effectScope();
    scope.run(() => useEventStream());
    await nextTick();
    const first = MockWebSocket.sockets[0];

    app.setServiceUrl("http://127.0.0.1:8081/");
    await nextTick();

    expect(MockWebSocket.sockets).toHaveLength(2);
    expect(app.eventStreamState).toBe("connecting");

    first.onclose?.();

    expect(app.eventStreamState).toBe("connecting");
    scope.stop();
  });
});
