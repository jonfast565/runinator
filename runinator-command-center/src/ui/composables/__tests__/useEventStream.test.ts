import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { effectScope, nextTick } from "vue";
import { useEventStream } from "../useEventStream";
import { setHttpAuthToken } from "../../../core/api/httpRuntime";
import { authService } from "../../../core/services";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useAuthStore } from "../../../ui/adapters/pinia/auth";
import { useResourcesStore } from "../../../ui/adapters/pinia/resources";

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
      setTimeout,
    });
    setHttpAuthToken(null);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("falls back when the event stream connect attempt times out", async () => {
    const app = useAppStore();
    app.setServiceUrl("http://127.0.0.1:8080/");

    const scope = effectScope();
    scope.run(() => {
      useEventStream();
    });
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
    scope.run(() => {
      useEventStream();
    });
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

  it("reconnects with the current access token when auth changes", async () => {
    const app = useAppStore();
    const auth = useAuthStore();
    app.setServiceUrl("http://127.0.0.1:8080/");

    const scope = effectScope();
    scope.run(() => {
      useEventStream();
    });
    await nextTick();

    await auth.applyAccessToken("org-token-2");
    await nextTick();

    expect(MockWebSocket.sockets).toHaveLength(2);
    expect(MockWebSocket.sockets[0].close).toHaveBeenCalled();
    expect(MockWebSocket.sockets[1].url).toBe("ws://127.0.0.1:8080/ws/events?token=org-token-2");
    scope.stop();
  });

  it("does not open a socket while auth is required but unauthenticated", async () => {
    const app = useAppStore();
    const auth = useAuthStore();
    // simulate an enabled-but-logged-out backend: a browser WS would only 401 on a ?token=-less connect.
    authService.resetForTests();
    authService.setState((state) => ({ ...state, required: true, authenticated: false }));
    app.setServiceUrl("http://127.0.0.1:8080/");

    const scope = effectScope();
    scope.run(() => {
      useEventStream();
    });
    await nextTick();

    // required + unauthenticated: no socket should be opened.
    expect(MockWebSocket.sockets).toHaveLength(0);

    // publish the token first (still unauthenticated → still no socket), then flip authenticated so
    // the single connect that follows already carries the token.
    await auth.applyAccessToken("org-token-9");
    await nextTick();
    expect(MockWebSocket.sockets).toHaveLength(0);

    authService.setState((state) => ({ ...state, authenticated: true }));
    await nextTick();

    expect(MockWebSocket.sockets).toHaveLength(1);
    expect(MockWebSocket.sockets[0].url).toBe("ws://127.0.0.1:8080/ws/events?token=org-token-9");
    scope.stop();
  });

  it("refreshes automation events when a workflow run changes", async () => {
    const app = useAppStore();
    const resources = useResourcesStore();
    const refreshResourcesFor = vi.spyOn(resources, "refreshResourcesFor").mockResolvedValue();
    app.setServiceUrl("http://127.0.0.1:8080/");
    app.activeTab = "Events";

    const scope = effectScope();
    scope.run(() => {
      useEventStream();
    });
    await nextTick();

    MockWebSocket.sockets[0].onmessage?.({
      data: JSON.stringify({ type: "workflow_run_changed", run_id: "run-1" }),
    });

    expect(refreshResourcesFor).toHaveBeenCalledWith("automation_events");
    scope.stop();
  });
});
