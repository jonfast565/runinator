import { wsBaseUrl } from "../../../core/api/httpRuntime";
import type {
  AuthStorage,
  PlatformAdapter,
  PlatformDialogs,
  ServiceDiscovery,
} from "../../../core/platform/types";
import { browserCommandRuntime } from "./runtime";
import { pickFileFromBrowser } from "./files";

const localStorageAuth: AuthStorage = {
  get(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* storage unavailable */
    }
  },
  remove(key) {
    try {
      localStorage.removeItem(key);
    } catch {
      /* storage unavailable */
    }
  },
};

const browserDialogs: PlatformDialogs = {
  confirm(message) {
    return typeof confirm === "function" ? confirm(message) : true;
  },
  prompt(message) {
    return typeof prompt === "function" ? prompt(message) : null;
  },
};

const browserServiceDiscovery: ServiceDiscovery = {
  isDesktop: () => false,
  webServiceUrl: () => wsBaseUrl(),
  getInitialStatus: () => Promise.resolve({ service_url: wsBaseUrl() || null }),
  startDiscovery: () => Promise.resolve(),
  listenServiceUrlChanged: () => Promise.resolve(() => undefined),
  listenDiscoveryError: () => Promise.resolve(() => undefined),
};

export function createBrowserPlatformAdapter(): PlatformAdapter {
  return {
    runtime: browserCommandRuntime,
    authStorage: localStorageAuth,
    dialogs: browserDialogs,
    serviceDiscovery: browserServiceDiscovery,
    filePicker: { pickFile: pickFileFromBrowser },
  };
}
