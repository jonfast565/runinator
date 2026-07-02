import {
  getServiceStatus,
  startServiceDiscovery,
  uploadArtifactFromPath,
  downloadArtifactToPath,
} from "../../../core/api/commandCenterApi";
import type { PlatformAdapter } from "../../../core/platform/types";
import { tauriCommandRuntime } from "./command-runtime";
import { isTauriRuntime, listenTauri } from "./runtime";
import {
  downloadArtifactInBrowser,
  pickFileFromBrowser,
  uploadArtifactFromBrowser,
} from "../browser/files";

const localStorageAuth = {
  get(key: string) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* storage unavailable */
    }
  },
  remove(key: string) {
    try {
      localStorage.removeItem(key);
    } catch {
      /* storage unavailable */
    }
  },
};

export function createTauriPlatformAdapter(): PlatformAdapter {
  const desktop = isTauriRuntime();

  return {
    runtime: tauriCommandRuntime,
    authStorage: localStorageAuth,
    dialogs: {
      confirm(message) {
        return typeof confirm === "function" ? confirm(message) : true;
      },
      prompt(message) {
        return typeof prompt === "function" ? prompt(message) : null;
      },
    },
    artifacts: {
      isDesktop: () => desktop,
      pickFile: pickFileFromBrowser,
      uploadFromPath: uploadArtifactFromPath,
      uploadFromBrowser: uploadArtifactFromBrowser,
      downloadInBrowser: downloadArtifactInBrowser,
      downloadToPath: downloadArtifactToPath,
    },
    serviceDiscovery: {
      isDesktop: () => desktop,
      webServiceUrl: () => tauriCommandRuntime.wsBaseUrl(),
      getInitialStatus: () => getServiceStatus(),
      startDiscovery: () => startServiceDiscovery().then(() => undefined),
      listenServiceUrlChanged: async (handler) =>
        listenTauri("service-url-changed", (event) => {
          const payload = event.payload as { service_url?: string | null } | null;
          handler(payload?.service_url ?? null);
        }),
      listenDiscoveryError: async (handler) =>
        listenTauri("service-discovery-error", (event) => {
          handler(String(event.payload));
        }),
    },
    filePicker: { pickFile: pickFileFromBrowser },
  };
}
