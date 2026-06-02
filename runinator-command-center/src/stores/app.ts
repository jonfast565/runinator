import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { AppTab, NavSection } from "../types/app";

export const navSections: NavSection[] = [
  {
    label: "Workspace",
    items: [
      { tab: "Workflows", label: "Workflows", icon: "workflow" },
      { tab: "Runs", label: "Runs", icon: "runs" }
    ]
  },
  {
    label: "Inbox",
    items: [
      { tab: "Approvals", label: "Approvals", icon: "approve", endpoint: "approvals" },
      { tab: "Notifications", label: "Notifications", icon: "bell", endpoint: "notifications" },
      { tab: "Feedback", label: "Feedback", icon: "message", endpoint: "feedback" }
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
      { tab: "ChangeSets", label: "Change Sets", icon: "list", endpoint: "change_sets" },
      { tab: "Workspaces", label: "Workspaces", icon: "folder", endpoint: "workspaces" },
      { tab: "Gates", label: "Gates", icon: "gate", endpoint: "gates" },
      { tab: "Secrets", label: "Config & Secrets", icon: "key" }
    ]
  }
];

export const tabs: AppTab[] = navSections.flatMap((section) => section.items.map((item) => item.tab));

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

export const useAppStore = defineStore("app", () => {
  const activeTab = ref<AppTab>("Workflows");
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
  const eventStreamLabel = computed(() => {
    switch (eventStreamState.value) {
      case "connected":
        return "WS live";
      case "connecting":
        return "WS connecting";
      case "fallback":
        return "Polling";
      default:
        return "WS offline";
    }
  });

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
    if (!url) eventStreamState.value = "disconnected";
  }

  function setEventStreamState(state: EventStreamState) {
    eventStreamState.value = state;
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
    normalizedSearch,
    lastRefreshText,
    statusLine,
    serviceLabel,
    serviceConnected,
    serviceBlocked,
    loadingMessage,
    eventStreamLabel,
    setStatus,
    setError,
    markBackendReachable,
    setServiceUrl,
    setEventStreamState,
    runOperation,
    dispose
  };
});
