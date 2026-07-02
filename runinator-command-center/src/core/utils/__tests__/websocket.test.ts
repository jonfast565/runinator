import { beforeEach, describe, expect, it } from "vitest";
import { setHttpAuthToken } from "../../api/httpRuntime";
import { buildWebSocketUrl } from "../websocket";

describe("websocket url utils", () => {
  beforeEach(() => {
    setHttpAuthToken(null);
  });

  it("builds a websocket url from a trailing slash service url", () => {
    expect(buildWebSocketUrl("http://127.0.0.1:8080/", "/ws/events")).toBe(
      "ws://127.0.0.1:8080/ws/events",
    );
  });

  it("preserves a configured base path", () => {
    expect(buildWebSocketUrl("http://127.0.0.1:8080/api/", "/ws/events")).toBe(
      "ws://127.0.0.1:8080/api/ws/events",
    );
  });

  it("uses secure websockets for https service urls", () => {
    expect(buildWebSocketUrl("https://example.test/api/", "/ws/events")).toBe(
      "wss://example.test/api/ws/events",
    );
  });

  it("appends the access token as a query param for browser websocket auth", () => {
    setHttpAuthToken("token-123");

    expect(buildWebSocketUrl("https://example.test/api/", "/ws/events")).toBe(
      "wss://example.test/api/ws/events?token=token-123",
    );
  });
});
