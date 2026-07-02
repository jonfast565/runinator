import { invokeViaHttp, apiBaseUrl, wsBaseUrl, setHttpAuthToken } from "../../../core/api/httpRuntime";
import type { CommandRuntime } from "../../../core/api/runtime";

export const browserCommandRuntime: CommandRuntime = {
  isTauri: () => false,
  invoke: invokeViaHttp,
  wsBaseUrl,
  apiBaseUrl,
};

export { setHttpAuthToken };
