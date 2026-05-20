import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { AppTab } from "../types/app";

export const tabs: AppTab[] = ["Workflows", "Runs", "Resources", "Secrets"];
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
