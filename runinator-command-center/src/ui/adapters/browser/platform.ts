import { wsBaseUrl } from "../../../core/api/httpRuntime";
import {
  getServiceStatus,
  startServiceDiscovery,
  uploadArtifactFromPath,
  downloadArtifactToPath,
} from "../../../core/api/commandCenterApi";
import type {
  AuthStorage,
  PlatformAdapter,
  PlatformDialogs,
  ServiceDiscovery,
} from "../../../core/platform/types";
import { browserCommandRuntime } from "./runtime";
import {
  downloadArtifactInBrowser,
  pickFileFromBrowser,
  uploadArtifactFromBrowser,
} from "./files";

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
  getInitialStatus: async () => ({ service_url: wsBaseUrl() || null }),
  startDiscovery: async () => undefined,
  listenServiceUrlChanged: async () => () => undefined,
  listenDiscoveryError: async () => () => undefined,
};

export function createBrowserPlatformAdapter(): PlatformAdapter {
  return {
    runtime: browserCommandRuntime,
    authStorage: localStorageAuth,
    dialogs: browserDialogs,
    artifacts: {
      isDesktop: () => false,
      pickFile: pickFileFromBrowser,
      uploadFromPath: uploadArtifactFromPath,
      uploadFromBrowser: uploadArtifactFromBrowser,
      downloadInBrowser: downloadArtifactInBrowser,
      downloadToPath: downloadArtifactToPath,
    },
    serviceDiscovery: browserServiceDiscovery,
    filePicker: { pickFile: pickFileFromBrowser },
  };
}
