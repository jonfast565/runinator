import { setCommandRuntime } from "./core/api/runtime";
import { apiBaseUrl, invokeViaHttp, wsBaseUrl } from "./core/api/httpRuntime";

setCommandRuntime({
  isTauri: () => false,
  invoke: invokeViaHttp,
  wsBaseUrl,
  apiBaseUrl,
});
