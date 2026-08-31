import { httpAuthToken } from "../api/httpRuntime";

export const WEBSOCKET_AUTH_PROTOCOL = "runinator-auth";
const WEBSOCKET_TOKEN_PROTOCOL_PREFIX = "runinator-token.";

export function buildWebSocketUrl(serviceUrl: string, routePath: string) {
  const url = new URL(serviceUrl);

  if (url.protocol === "http:") {
    url.protocol = "ws:";
  } else if (url.protocol === "https:") {
    url.protocol = "wss:";
  } else if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error(`Unsupported WebSocket base protocol: ${url.protocol}`);
  }

  const basePath = url.pathname.replace(/\/+$/, "");
  const route = routePath.replace(/^\/+/, "");
  url.pathname = `${basePath}/${route}`.replace(/\/{2,}/g, "/");
  url.search = "";
  url.hash = "";
  return url.toString();
}

/**
 * Browser WebSockets cannot attach an Authorization header. Offer the bearer token as a dedicated
 * subprotocol instead of a URL query parameter so it is not copied into console and access logs.
 */
export function buildWebSocketProtocols(): string[] {
  const token = httpAuthToken();
  return token ? [WEBSOCKET_AUTH_PROTOCOL, `${WEBSOCKET_TOKEN_PROTOCOL_PREFIX}${token}`] : [];
}
