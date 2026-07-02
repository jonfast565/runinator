import { invoke } from "@tauri-apps/api/core";
import { invokeViaHttp, apiBaseUrl, wsBaseUrl, setHttpAuthToken } from "../../../core/api/httpRuntime";
import { isTauriRuntime as detectTauri } from "./runtime";
import type { CommandRuntime } from "../../../core/api/runtime";

export { isTauriRuntime, listenTauri } from "./runtime";

export const tauriCommandRuntime: CommandRuntime = {
  isTauri: detectTauri,
  invoke<T>(name: string, args?: Record<string, unknown>) {
    if (detectTauri()) {
      return invoke<T>(name, args);
    }

    return invokeViaHttp<T>(name, args);
  },
  wsBaseUrl,
  apiBaseUrl,
};

export { setHttpAuthToken };
