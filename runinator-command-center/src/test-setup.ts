import { setCommandRuntime } from "./core/api/runtime";
import { apiBaseUrl, invokeViaHttp, wsBaseUrl } from "./core/api/httpRuntime";
import { createBrowserPlatformAdapter } from "./ui/adapters/browser/platform";
import { setPlatformAdapter } from "./core/platform";

setCommandRuntime({
  isTauri: () => false,
  invoke: invokeViaHttp,
  wsBaseUrl,
  apiBaseUrl,
});

setPlatformAdapter(createBrowserPlatformAdapter());
