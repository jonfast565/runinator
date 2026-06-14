import { httpAuthToken } from "../api/httpRuntime";

export function buildWebSocketUrl(serviceUrl: string, routePath: string) {
  const url = new URL(serviceUrl);
  if (url.protocol === "http:") url.protocol = "ws:";
  else if (url.protocol === "https:") url.protocol = "wss:";
  else if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error(`Unsupported WebSocket base protocol: ${url.protocol}`);
  }

  const basePath = url.pathname.replace(/\/+$/, "");
  const route = routePath.replace(/^\/+/, "");
  url.pathname = `${basePath}/${route}`.replace(/\/{2,}/g, "/");
  url.search = "";
  url.hash = "";
  // browsers can't set headers on a WebSocket upgrade, so the access token rides as a query param.
  const token = httpAuthToken();
  if (token) url.searchParams.set("token", token);
  return url.toString();
}
