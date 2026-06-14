import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { fetchReplicas as fetchReplicasApi } from "../api/commandCenterApi";
import { isTauriRuntime } from "../api/tauriRuntime";
import type { ReplicaCounts, ReplicaRecord } from "../types/models";
import type { AppTab, NavSection } from "../types/app";

export const navSections: NavSection[] = [
  {
    label: "Workspace",
    items: [
      { tab: "Dev", label: "Dev", icon: "debug", desktopOnly: true },
      { tab: "Workflows", label: "Workflows", icon: "workflow" },
      { tab: "Runs", label: "Runs", icon: "runs" },
      { tab: "Providers", label: "Providers", icon: "box" },
      { tab: "Replicas", label: "Replicas", icon: "list" }
    ]
  },
  {
    label: "Inbox",
    items: [
      { tab: "Approvals", label: "Approvals", icon: "approve", endpoint: "approvals" },
      { tab: "Notifications", label: "Notifications", icon: "bell", endpoint: "notifications" }
    ]
  },
  {
    label: "Data",
    items: [
      { tab: "Artifacts", label: "Artifacts", icon: "box", endpoint: "artifacts" },
      { tab: "ExternalItems", label: "External Items", icon: "tag", endpoint: "external_items" },
      { tab: "Events", label: "Events", icon: "flag", endpoint: "automation_events" }
    ]
  },
  {
    label: "Other",
    items: [
      { tab: "Gates", label: "Gates", icon: "gate" },
      { tab: "Secrets", label: "Config & Secrets", icon: "key" }
    ]
  }
];

export const tabs: AppTab[] = navSections.flatMap((section) => section.items.map((item) => item.tab));

// nav sections with desktop-only items dropped when running in the hosted web app.
export function visibleNavSections(): NavSection[] {
  if (isTauriRuntime()) return navSections;
  return navSections
    .map((section) => ({ ...section, items: section.items.filter((item) => !item.desktopOnly) }))
    .filter((section) => section.items.length > 0);
}

const navItemByTab = new Map(
  navSections.flatMap((section) => section.items.map((item) => [item.tab, item] as const))
);

export function navItemForTab(tab: AppTab) {
  return navItemByTab.get(tab);
}

export function endpointForTab(tab: AppTab): string | undefined {
  return navItemByTab.get(tab)?.endpoint;
}

export function isResourceTab(tab: AppTab): boolean {
  const endpoint = endpointForTab(tab);
  if (!endpoint) return false;
  // Artifacts and Notifications have dedicated stores; treat them separately.
  return endpoint !== "artifacts" && endpoint !== "notifications";
}
export type EventStreamState = "disconnected" | "connecting" | "connected" | "fallback";

const SIDEBAR_COLLAPSED_KEY = "command-center.sidebar.collapsed";

function readSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
  } catch {
    return false;
  }
}

export const useAppStore = defineStore("app", () => {
  const activeTab = ref<AppTab>("Workflows");
  const sidebarCollapsed = ref(readSidebarCollapsed());
  const serviceUrl = ref<string | null>(null);
  const backendReachable = ref(false);
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
  let statusTimer = 0;

  const normalizedSearch = computed(() => searchQuery.value.trim().toLowerCase());
  const lastRefreshText = computed(() => (lastRefreshAt.value ? lastRefreshAt.value.toLocaleTimeString() : "-"));
  const statusLine = computed(() => {
    if (errorText.value) return `Error: ${errorText.value}`;
    if (loading.value || opLabel.value) return `${opLabel.value || "Working"}...`;
    return statusText.value || "Ready.";
  });
  const serviceLabel = computed(() => serviceUrl.value ?? (backendReachable.value ? "Service reachable" : "No service discovered"));
  const serviceConnected = computed(() => Boolean(serviceUrl.value || backendReachable.value));
  const serviceBlocked = computed(() => initialLoading.value || (!errorText.value && !serviceConnected.value));
  const loadingMessage = computed(() => (serviceConnected.value ? "Loading Runinator..." : "Waiting for Runinator service..."));
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
  const liveReplicaCount = computed(() => replicas.value.filter((replica) => replica.status === "live").length);
  const hasReplicaState = computed(() => replicas.value.length > 0);

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;
    try {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed.value));
    } catch {
      /* storage unavailable; collapse state is then memory-only */
    }
  }

  function setStatus(text: string) {
    statusText.value = text;
    errorText.value = "";
    lastRefreshAt.value = new Date();
    window.clearTimeout(statusTimer);
    statusTimer = window.setTimeout(() => (statusText.value = ""), 5000);
  }

  function setError(text: string) {
    errorText.value = text;
    statusText.value = "";
    initialLoading.value = false;
  }

  function markBackendReachable() {
    backendReachable.value = true;
  }

  function setServiceUrl(url: string | null | undefined) {
    if (url === undefined) return;
    serviceUrl.value = url;
    backendReachable.value = Boolean(url);
    if (url) errorText.value = "";
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
      workers: nextReplicas.filter((replica) => replica.replica_type === "worker" && replica.status === "live").length,
      wakers: nextReplicas.filter((replica) => replica.replica_type === "waker" && replica.status === "live").length,
      webservices: nextReplicas.filter((replica) => replica.replica_type === "webservice" && replica.status === "live").length
    };
  }

  function clearReplicaState() {
    replicas.value = [];
    replicaCounts.value = { workers: 0, wakers: 0, webservices: 0 };
  }

  async function refreshReplicas() {
    const response = await fetchReplicasApi();
    const nextReplicas = [...(response.replicas ?? [])].sort((left, right) => {
      const typeOrder = replicaKindOrder(left.replica_type) - replicaKindOrder(right.replica_type);
      if (typeOrder !== 0) return typeOrder;
      const statusOrder = replicaStatusOrder(left.status) - replicaStatusOrder(right.status);
      if (statusOrder !== 0) return statusOrder;
      return replicaLabel(left).localeCompare(replicaLabel(right));
    });
    setReplicaState(nextReplicas, response.counts);
  }

  async function runOperation<T>(label: string, operation: () => Promise<T>): Promise<T> {
    loading.value = true;
    opLabel.value = label;
    errorText.value = "";
    try {
      const result = await operation();
      markBackendReachable();
      return result;
    } catch (error) {
      setError(String(error));
      throw error;
    } finally {
      loading.value = false;
      opLabel.value = "";
    }
  }

  function dispose() {
    window.clearTimeout(statusTimer);
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
    normalizedSearch,
    lastRefreshText,
    statusLine,
    serviceLabel,
    serviceConnected,
    serviceBlocked,
    loadingMessage,
    isRealtime,
    eventStreamLabel,
    liveReplicaCount,
    hasReplicaState,
    setStatus,
    setError,
    markBackendReachable,
    setServiceUrl,
    setEventStreamState,
    setReplicaState,
    clearReplicaState,
    refreshReplicas,
    runOperation,
    dispose
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

function replicaLabel(replica: { display_name?: string | null; host?: string | null; instance_id: string }) {
  return replica.display_name || replica.host || replica.instance_id;
}
