import { fetchReplicas as fetchReplicasApi } from "../api/commandCenterApi";
import type { AppTab } from "../navigation/app";
import {
  readSidebarCollapsed,
  readStoredDefaultTab,
  tabs,
} from "../navigation/nav-config";
import type { ReplicaCounts, ReplicaRecord } from "../domain/models";
import { createStore } from "./event-bus";

export type EventStreamState = "disconnected" | "connecting" | "connected" | "fallback";
export type ToastKind = "info" | "loading" | "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

export interface AppState {
  activeTab: AppTab;
  sidebarCollapsed: boolean;
  mobileNavOpen: boolean;
  serviceUrl: string | null;
  backendReachable: boolean;
  outageDismissed: boolean;
  initialLoading: boolean;
  loading: boolean;
  opLabel: string;
  statusText: string;
  errorText: string;
  searchQuery: string;
  lastRefreshAt: Date | null;
  eventStreamState: EventStreamState;
  replicaCounts: ReplicaCounts;
  replicas: ReplicaRecord[];
  toasts: Toast[];
}

const TOAST_TIMEOUTS: Record<ToastKind, number | null> = {
  info: 5000,
  loading: null,
  success: 5000,
  error: 8000,
};

const MAX_TOASTS = 4;
const SIDEBAR_COLLAPSED_KEY = "command-center.sidebar.collapsed";

export function isNetworkError(error: unknown): boolean {
  if (error instanceof TypeError) {
    return true;
  }

  const message = String((error as { message?: unknown }).message ?? error).toLowerCase();
  return (
    message.includes("failed to fetch") ||
    message.includes("load failed") ||
    message.includes("networkerror") ||
    message.includes("network request failed")
  );
}

export function createAppService() {
  const store = createStore<AppState>({
    activeTab: readStoredDefaultTab(),
    sidebarCollapsed: readSidebarCollapsed(),
    mobileNavOpen: false,
    serviceUrl: null,
    backendReachable: false,
    outageDismissed: false,
    initialLoading: true,
    loading: false,
    opLabel: "",
    statusText: "",
    errorText: "",
    searchQuery: "",
    lastRefreshAt: null,
    eventStreamState: "disconnected",
    replicaCounts: { workers: 0, wakers: 0, webservices: 0, background: 0 },
    replicas: [],
    toasts: [],
  });

  let statusTimer = 0;
  let toastSeq = 0;
  const toastTimers = new Map<number, number>();

  function dismissToast(id: number) {
    store.setState((state) => ({
      ...state,
      toasts: state.toasts.filter((toast) => toast.id !== id),
    }));
    const timer = toastTimers.get(id);

    if (timer) {
      window.clearTimeout(timer);
      toastTimers.delete(id);
    }
  }

  function pushToast(kind: ToastKind, text: string): number {
    const id = ++toastSeq;
    store.setState((state) => ({
      ...state,
      toasts: [...state.toasts, { id, kind, text }].slice(-MAX_TOASTS),
    }));
    const timeout = TOAST_TIMEOUTS[kind];

    if (timeout !== null) {
      toastTimers.set(
        id,
        window.setTimeout(() => {
          dismissToast(id);
        }, timeout),
      );
    }

    return id;
  }

  return {
    ...store,
    resetForTests() {
      if (typeof window !== "undefined") {
        window.clearTimeout(statusTimer);

        for (const timer of toastTimers.values()) {
          window.clearTimeout(timer);
        }
      }

      toastTimers.clear();
      store.setState(() => ({
        activeTab: readStoredDefaultTab(),
        sidebarCollapsed: readSidebarCollapsed(),
        mobileNavOpen: false,
        serviceUrl: null,
        backendReachable: false,
        outageDismissed: false,
        initialLoading: true,
        loading: false,
        opLabel: "",
        statusText: "",
        errorText: "",
        searchQuery: "",
        lastRefreshAt: null,
        eventStreamState: "disconnected",
        replicaCounts: { workers: 0, wakers: 0, webservices: 0, background: 0 },
        replicas: [],
        toasts: [],
      }));
    },
    get normalizedSearch() {
      return store.getState().searchQuery.trim().toLowerCase();
    },
    setActiveTab(tab: AppTab) {
      store.setState((state) => ({ ...state, activeTab: tab, mobileNavOpen: false }));
    },
    openMobileNav() {
      store.setState((state) => ({ ...state, mobileNavOpen: true }));
    },
    closeMobileNav() {
      store.setState((state) => ({ ...state, mobileNavOpen: false }));
    },
    toggleMobileNav() {
      store.setState((state) => ({ ...state, mobileNavOpen: !state.mobileNavOpen }));
    },
    toggleSidebar() {
      store.setState((state) => {
        const sidebarCollapsed = !state.sidebarCollapsed;

        try {
          localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed));
        } catch {
          /* storage unavailable */
        }

        return { ...state, sidebarCollapsed };
      });
    },
    setStatus(text: string) {
      store.setState((state) => ({
        ...state,
        statusText: text,
        errorText: "",
        lastRefreshAt: new Date(),
      }));
      window.clearTimeout(statusTimer);
      statusTimer = window.setTimeout(() => {
        store.setState((state) => ({ ...state, statusText: "" }));
      }, 5000);
      pushToast("success", text);
    },
    setError(text: string) {
      store.setState((state) => ({
        ...state,
        errorText: text,
        statusText: "",
        initialLoading: false,
      }));
      pushToast("error", text);
    },
    pushToast,
    dismissToast,
    clearToasts() {
      for (const timer of toastTimers.values()) {
        window.clearTimeout(timer);
      }

      toastTimers.clear();
      store.setState((state) => ({ ...state, toasts: [] }));
    },
    markBackendReachable() {
      store.setState((state) => ({ ...state, backendReachable: true, outageDismissed: false }));
    },
    markBackendUnreachable() {
      store.setState((state) => ({ ...state, backendReachable: false }));
    },
    dismissOutageBanner() {
      store.setState((state) => ({ ...state, outageDismissed: true }));
    },
    setServiceUrl(url: string | null | undefined) {
      if (url === undefined) {
        return;
      }

      store.setState((state) => ({
        ...state,
        serviceUrl: url,
        backendReachable: Boolean(url),
        errorText: url ? "" : state.errorText,
        eventStreamState: url ? state.eventStreamState : "disconnected",
        replicas: url ? state.replicas : [],
        replicaCounts: url ? state.replicaCounts : { workers: 0, wakers: 0, webservices: 0, background: 0 },
      }));
    },
    setEventStreamState(state: EventStreamState) {
      store.setState((current) => ({ ...current, eventStreamState: state }));
    },
    setReplicaState(nextReplicas: ReplicaRecord[], nextCounts?: ReplicaCounts | null) {
      store.setState((state) => ({
        ...state,
        replicas: [...nextReplicas],
        replicaCounts: nextCounts ?? {
          workers: nextReplicas.filter(
            (replica) => replica.replica_type === "worker" && replica.status === "live",
          ).length,
          wakers: nextReplicas.filter(
            (replica) => replica.replica_type === "waker" && replica.status === "live",
          ).length,
          webservices: nextReplicas.filter(
            (replica) => replica.replica_type === "webservice" && replica.status === "live",
          ).length,
          background: nextReplicas.filter(
            (replica) => replica.replica_type === "background" && replica.status === "live",
          ).length,
        },
      }));
    },
    clearReplicaState() {
      store.setState((state) => ({
        ...state,
        replicas: [],
        replicaCounts: { workers: 0, wakers: 0, webservices: 0, background: 0 },
      }));
    },
    setInitialLoading(value: boolean) {
      store.setState((state) => ({ ...state, initialLoading: value }));
    },
    setSearchQuery(query: string) {
      store.setState((state) => ({ ...state, searchQuery: query }));
    },
    async refreshReplicas() {
      const response = await fetchReplicasApi();
      const nextReplicas = [...response.replicas].sort((left, right) => {
        const typeOrder = replicaKindOrder(left.replica_type) - replicaKindOrder(right.replica_type);

        if (typeOrder !== 0) {
          return typeOrder;
        }

        const statusOrder = replicaStatusOrder(left.status) - replicaStatusOrder(right.status);

        if (statusOrder !== 0) {
          return statusOrder;
        }

        return replicaLabel(left).localeCompare(replicaLabel(right));
      });
      store.setState((state) => ({
        ...state,
        replicas: nextReplicas,
        replicaCounts: response.counts,
      }));
    },
    async runOperation<T>(label: string, operation: () => Promise<T>): Promise<T> {
      store.setState((state) => ({ ...state, loading: true, opLabel: label, errorText: "" }));
      const toastId = pushToast("loading", `${label}...`);

      try {
        const result = await operation();
        store.setState((state) => ({ ...state, backendReachable: true, outageDismissed: false }));
        return result;
      } catch (error) {
        if (isNetworkError(error)) {
          store.setState((state) => ({ ...state, backendReachable: false }));
        }

        const message = String(error);
        store.setState((state) => ({
          ...state,
          errorText: message,
          statusText: "",
          initialLoading: false,
        }));
        pushToast("error", message);
        throw error;
      } finally {
        store.setState((state) => ({ ...state, loading: false, opLabel: "" }));
        dismissToast(toastId);
      }
    },
    dispose() {
      window.clearTimeout(statusTimer);

      for (const timer of toastTimers.values()) {
        window.clearTimeout(timer);
      }

      toastTimers.clear();
    },
  };
}

export type AppService = ReturnType<typeof createAppService>;

function replicaKindOrder(kind: string) {
  switch (kind) {
    case "webservice":
      return 0;
    case "worker":
      return 1;
    case "waker":
      return 2;
    default:
      return 3;
  }
}

function replicaStatusOrder(status: string) {
  switch (status) {
    case "live":
      return 0;
    case "stale":
      return 1;
    case "offline":
      return 2;
    default:
      return 3;
  }
}

function replicaLabel(replica: {
  display_name?: string | null;
  host?: string | null;
  instance_id: string;
}) {
  return replica.display_name ?? replica.host ?? replica.instance_id;
}

export { tabs };
