import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { fetchReplicas as fetchReplicasApi } from "../api/commandCenterApi";
import { isTauriRuntime } from "../api/tauriRuntime";
import { useAuthStore } from "./auth";
import type { ReplicaCounts, ReplicaRecord } from "../types/models";
import type { AppTab, NavSection } from "../types/app";

export const navSections: NavSection[] = [
  {
    label: "Workspace",
    items: [
      { tab: "Dev", label: "Dev", icon: "debug", desktopOnly: true },
      {
        tab: "Workflows",
        label: "Workflows",
        icon: "workflow",
        searchPlaceholder: "Search workflows",
      },
      { tab: "Runs", label: "Runs", icon: "runs", searchPlaceholder: "Search runs" },
      { tab: "Providers", label: "Providers", icon: "box", searchPlaceholder: "Search providers" },
      { tab: "Replicas", label: "Replicas", icon: "list", searchPlaceholder: "Search replicas" },
    ],
  },
  {
    label: "Inbox",
    items: [
      {
        tab: "Approvals",
        label: "Approvals",
        icon: "approve",
        endpoint: "approvals",
        searchPlaceholder: "Search approvals",
      },
      {
        tab: "Notifications",
        label: "Notifications",
        icon: "bell",
        endpoint: "notifications",
        searchPlaceholder: "Search notifications",
      },
    ],
  },
  {
    label: "Data",
    items: [
      {
        tab: "Artifacts",
        label: "Artifacts",
        icon: "box",
        endpoint: "artifacts",
        searchPlaceholder: "Search artifacts",
      },
      {
        tab: "ExternalItems",
        label: "External Items",
        icon: "tag",
        endpoint: "external_items",
        searchPlaceholder: "Search external items",
      },
      {
        tab: "Events",
        label: "Events",
        icon: "flag",
        endpoint: "automation_events",
        searchPlaceholder: "Search events",
      },
    ],
  },
  {
    label: "Other",
    items: [
      { tab: "Gates", label: "Gates", icon: "gate", searchPlaceholder: "Search gates" },
      { tab: "Configs", label: "Configs", icon: "settings", searchPlaceholder: "Search configs" },
      { tab: "Secrets", label: "Secrets", icon: "key", searchPlaceholder: "Search secrets" },
    ],
  },
  {
    label: "Organization",
    items: [
      { tab: "Organization", label: "Organization", icon: "shield" },
      { tab: "OrgResources", label: "Resources & Billing", icon: "box" },
    ],
  },
  {
    label: "Admin",
    items: [
      { tab: "AdminSettings", label: "Settings", icon: "settings", adminOnly: true },
      {
        tab: "Permissions",
        label: "Permissions",
        icon: "shield",
        adminOnly: true,
        searchPlaceholder: "Search users & teams",
      },
      { tab: "DeadLetters", label: "Dead Letters", icon: "flag", adminOnly: true },
      { tab: "AuditLog", label: "Audit Log", icon: "list", adminOnly: true },
    ],
  },
];

export const tabs: AppTab[] = navSections.flatMap((section) =>
  section.items.map((item) => item.tab),
);

// nav sections with desktop-only items dropped when running in the hosted web app.
export function visibleNavSections(): NavSection[] {
  const auth = useAuthStore();
  const canSeeAdmin = !auth.required || auth.user?.is_admin === true;
  const sections = navSections
    .map((section) => ({
      ...section,
      items: section.items.filter((item) => !item.adminOnly || canSeeAdmin),
    }))
    .filter((section) => section.items.length > 0);

  if (isTauriRuntime()) {
    return sections;
  }

  return sections
    .map((section) => ({ ...section, items: section.items.filter((item) => !item.desktopOnly) }))
    .filter((section) => section.items.length > 0);
}

const navItemByTab = new Map(
  navSections.flatMap((section) => section.items.map((item) => [item.tab, item] as const)),
);

export function navItemForTab(tab: AppTab) {
  return navItemByTab.get(tab);
}

export function endpointForTab(tab: AppTab): string | undefined {
  return navItemByTab.get(tab)?.endpoint;
}

export function isResourceTab(tab: AppTab): boolean {
  const endpoint = endpointForTab(tab);

  if (!endpoint) {
    return false;
  }

  // Artifacts and Notifications have dedicated stores; treat them separately.
  return endpoint !== "artifacts" && endpoint !== "notifications";
}

export type EventStreamState = "disconnected" | "connecting" | "connected" | "fallback";

// transient feedback toasts. "loading"/"info" render neutral, never success-green.
export type ToastKind = "info" | "loading" | "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

// how long each kind stays before auto-dismissing (null = sticky until replaced/dismissed).
const TOAST_TIMEOUTS: Record<ToastKind, number | null> = {
  info: 5000,
  loading: null,
  success: 5000,
  error: 8000,
};

// cap the visible stack so a burst of operations can't bury the screen.
const MAX_TOASTS = 4;

const SIDEBAR_COLLAPSED_KEY = "command-center.sidebar.collapsed";
const DEFAULT_TAB_KEY = "command-center.defaultTab";

function readSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
  } catch {
    return false;
  }
}

// distinguish a fetch that never reached the backend (network/proxy down) from a backend response
// that carried an error status. browsers report the former as a TypeError ("Failed to fetch",
// "Load failed", "NetworkError ..."); our http runtime throws a plain Error for non-ok responses.
export function isNetworkError(error: unknown): boolean {
  if (error instanceof TypeError) {
    return true;
  }

  const message = String(
    (error as { message?: unknown }).message ?? error,
  ).toLowerCase();
  return (
    message.includes("failed to fetch") ||
    message.includes("load failed") ||
    message.includes("networkerror") ||
    message.includes("network request failed")
  );
}

function readStoredDefaultTab(): AppTab {
  try {
    const stored = localStorage.getItem(DEFAULT_TAB_KEY);

    if (stored && (tabs as string[]).includes(stored)) {
      return stored as AppTab;
    }
  } catch {
    // storage unavailable; use default.
  }

  return "Workflows";
}

export const useAppStore = defineStore("app", () => {
  const activeTab = ref<AppTab>(readStoredDefaultTab());
  const sidebarCollapsed = ref(readSidebarCollapsed());
  const serviceUrl = ref<string | null>(null);
  const backendReachable = ref(false);
  // set when the user dismisses the outage banner; reset once reachability returns.
  const outageDismissed = ref(false);
  const initialLoading = ref(true);
  const loading = ref(false);
  const opLabel = ref("");
  const statusText = ref("");
  const errorText = ref("");
  const searchQuery = ref("");
  const lastRefreshAt = ref<Date | null>(null);
  const eventStreamState = ref<EventStreamState>("disconnected");
  const replicaCounts = ref<ReplicaCounts>({ workers: 0, wakers: 0, webservices: 0 });
  const replicas = ref<ReplicaRecord[]>([]);
  const toasts = ref<Toast[]>([]);
  let statusTimer = 0;
  let toastSeq = 0;
  const toastTimers = new Map<number, number>();

  const normalizedSearch = computed(() => searchQuery.value.trim().toLowerCase());
  const lastRefreshText = computed(() =>
    lastRefreshAt.value ? lastRefreshAt.value.toLocaleTimeString() : "-",
  );
  const statusLine = computed(() => {
    if (errorText.value) {
      return `Error: ${errorText.value}`;
    }

    if (loading.value || opLabel.value) {
      return `${opLabel.value || "Working"}...`;
    }

    return statusText.value || "Ready.";
  });
  const serviceLabel = computed(
    () =>
      serviceUrl.value ?? (backendReachable.value ? "Service reachable" : "No service discovered"),
  );
  // whether we know where the backend lives (discovery / web-mode origin), independent of live reachability.
  const serviceKnown = computed(() => Boolean(serviceUrl.value));
  // the connection tag reflects live reachability: it goes red as soon as a request fails at the network level.
  const serviceConnected = computed(() => backendReachable.value);
  // full-screen gate is for bootstrap only; a transient outage uses the slim banner plus disabled content.
  const serviceBlocked = computed(
    () => initialLoading.value || (!errorText.value && !serviceKnown.value),
  );
  const outageActive = computed(
    () => serviceKnown.value && !backendReachable.value && !initialLoading.value,
  );
  const interactionsDisabled = computed(() => serviceBlocked.value || outageActive.value);
  // banner shows once we know the service but can no longer reach it, until reachability returns or the user dismisses it.
  const showOutageBanner = computed(() => outageActive.value && !outageDismissed.value);
  const loadingMessage = computed(() =>
    serviceConnected.value ? "Loading Runinator..." : "Waiting for Runinator service...",
  );
  const isRealtime = computed(() => eventStreamState.value === "connected");
  const eventStreamLabel = computed(() => {
    switch (eventStreamState.value) {
      case "connected":
        return "Live updates";
      case "connecting":
        return "Opening stream";
      case "fallback":
        return "Refresh mode";
      default:
        return "Updates paused";
    }
  });
  const liveReplicaCount = computed(
    () => replicas.value.filter((replica) => replica.status === "live").length,
  );
  const hasReplicaState = computed(() => replicas.value.length > 0);

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;

    try {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed.value));
    } catch {
      /* storage unavailable; collapse state is then memory-only */
    }
  }

  // push a transient toast onto the stack; returns its id so callers can dismiss it early.
  function pushToast(kind: ToastKind, text: string): number {
    const id = ++toastSeq;
    toasts.value = [...toasts.value, { id, kind, text }].slice(-MAX_TOASTS);
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

  function dismissToast(id: number) {
    toasts.value = toasts.value.filter((toast) => toast.id !== id);
    const timer = toastTimers.get(id);

    if (timer) {
      window.clearTimeout(timer);
      toastTimers.delete(id);
    }
  }

  function clearToasts() {
    for (const timer of toastTimers.values()) {
      window.clearTimeout(timer);
    }

    toastTimers.clear();
    toasts.value = [];
  }

  function setStatus(text: string) {
    statusText.value = text;
    errorText.value = "";
    lastRefreshAt.value = new Date();
    window.clearTimeout(statusTimer);
    statusTimer = window.setTimeout(() => (statusText.value = ""), 5000);
    pushToast("success", text);
  }

  function setError(text: string) {
    errorText.value = text;
    statusText.value = "";
    initialLoading.value = false;
    pushToast("error", text);
  }

  function markBackendReachable() {
    backendReachable.value = true;
    // reachability returned; allow the banner to show again on the next outage.
    outageDismissed.value = false;
  }

  function markBackendUnreachable() {
    backendReachable.value = false;
  }

  function dismissOutageBanner() {
    outageDismissed.value = true;
  }

  function setServiceUrl(url: string | null | undefined) {
    if (url === undefined) {
      return;
    }

    serviceUrl.value = url;
    backendReachable.value = Boolean(url);

    if (url) {
      errorText.value = "";
    }

    if (!url) {
      eventStreamState.value = "disconnected";
      clearReplicaState();
    }
  }

  function setEventStreamState(state: EventStreamState) {
    eventStreamState.value = state;
  }

  function setReplicaState(nextReplicas: ReplicaRecord[], nextCounts?: ReplicaCounts | null) {
    replicas.value = [...nextReplicas];
    replicaCounts.value = nextCounts ?? {
      workers: nextReplicas.filter(
        (replica) => replica.replica_type === "worker" && replica.status === "live",
      ).length,
      wakers: nextReplicas.filter(
        (replica) => replica.replica_type === "waker" && replica.status === "live",
      ).length,
      webservices: nextReplicas.filter(
        (replica) => replica.replica_type === "webservice" && replica.status === "live",
      ).length,
    };
  }

  function clearReplicaState() {
    replicas.value = [];
    replicaCounts.value = { workers: 0, wakers: 0, webservices: 0 };
  }

  async function refreshReplicas() {
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
    setReplicaState(nextReplicas, response.counts);
  }

  async function runOperation<T>(label: string, operation: () => Promise<T>): Promise<T> {
    loading.value = true;
    opLabel.value = label;
    errorText.value = "";
    const toastId = pushToast("loading", `${label}...`);

    try {
      const result = await operation();
      markBackendReachable();
      return result;
    } catch (error) {
      // a network-level failure (fetch rejected) means the backend/proxy is unreachable, not that
      // it returned an error; flip the reachability signal so the tag + banner reflect the outage.
      if (isNetworkError(error)) {
        markBackendUnreachable();
      }

      setError(String(error));
      throw error;
    } finally {
      loading.value = false;
      opLabel.value = "";
      dismissToast(toastId);
    }
  }

  function dispose() {
    window.clearTimeout(statusTimer);
    clearToasts();
  }

  return {
    activeTab,
    sidebarCollapsed,
    toggleSidebar,
    serviceUrl,
    backendReachable,
    initialLoading,
    loading,
    opLabel,
    statusText,
    errorText,
    searchQuery,
    lastRefreshAt,
    eventStreamState,
    replicaCounts,
    replicas,
    toasts,
    normalizedSearch,
    lastRefreshText,
    statusLine,
    serviceLabel,
    serviceKnown,
    serviceConnected,
    serviceBlocked,
    interactionsDisabled,
    showOutageBanner,
    loadingMessage,
    isRealtime,
    eventStreamLabel,
    liveReplicaCount,
    hasReplicaState,
    setStatus,
    setError,
    pushToast,
    dismissToast,
    clearToasts,
    markBackendReachable,
    markBackendUnreachable,
    dismissOutageBanner,
    setServiceUrl,
    setEventStreamState,
    setReplicaState,
    clearReplicaState,
    refreshReplicas,
    runOperation,
    dispose,
  };
});

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
