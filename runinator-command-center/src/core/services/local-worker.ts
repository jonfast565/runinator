import {
  type LocalWorkerConfig,
  type LocalWorkerStatus,
  localWorkerStatus,
  startLocalWorker,
  stopLocalWorker,
} from "../api/commandCenterApi";
import { isTauriRuntime } from "../api/runtime";
import { errorMessage } from "../utils/format";
import { createStore } from "./event-bus";

export interface LocalWorkerState {
  status: LocalWorkerStatus;
  busy: boolean;
  error: string | null;
}

export function createLocalWorkerService() {
  const supported = isTauriRuntime();
  const store = createStore<LocalWorkerState>({
    status: {
      running: false,
      replica_id: null,
      root: null,
      broker_url: null,
    },
    busy: false,
    error: null,
  });

  const service = {
    ...store,
    supported,
    async refresh() {
      if (!supported) {
        return;
      }

      try {
        const status = await localWorkerStatus();
        store.setState((state) => ({ ...state, status, error: null }));
      } catch (err) {
        store.setState((state) => ({
          ...state,
          error: errorMessage(err) || "Failed to read local worker status",
        }));
      }
    },
    async start(config: LocalWorkerConfig) {
      if (!supported) {
        return;
      }

      store.setState((state) => ({ ...state, busy: true, error: null }));

      try {
        const status = await startLocalWorker(config);
        store.setState((state) => ({ ...state, status, busy: false }));
      } catch (err) {
        store.setState((state) => ({
          ...state,
          busy: false,
          error: errorMessage(err) || "Failed to start local worker",
        }));
      }
    },
    async stop() {
      if (!supported) {
        return;
      }

      store.setState((state) => ({ ...state, busy: true, error: null }));

      try {
        const status = await stopLocalWorker();
        store.setState((state) => ({ ...state, status, busy: false }));
      } catch (err) {
        store.setState((state) => ({
          ...state,
          busy: false,
          error: errorMessage(err) || "Failed to stop local worker",
        }));
      }
    },
  };

  return service;
}

export type LocalWorkerService = ReturnType<typeof createLocalWorkerService>;
