import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { AppTab } from "../types/app";

export const tabs: AppTab[] = ["Tasks", "Runs", "Workflows", "Resources"];

export const useAppStore = defineStore("app", () => {
  const activeTab = ref<AppTab>("Tasks");
  const serviceUrl = ref<string | null>(null);
  const loading = ref(false);
  const opLabel = ref("");
  const statusText = ref("");
  const errorText = ref("");
  const searchQuery = ref("");
  const lastRefreshAt = ref<Date | null>(null);
  let statusTimer = 0;

  const normalizedSearch = computed(() => searchQuery.value.trim().toLowerCase());
  const lastRefreshText = computed(() => (lastRefreshAt.value ? lastRefreshAt.value.toLocaleTimeString() : "-"));
  const statusLine = computed(() => {
    if (errorText.value) return `Error: ${errorText.value}`;
    if (loading.value || opLabel.value) return `${opLabel.value || "Working"}...`;
    return statusText.value || "Ready.";
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
  }

  async function runOperation<T>(label: string, operation: () => Promise<T>): Promise<T> {
    loading.value = true;
    opLabel.value = label;
    errorText.value = "";
    try {
      return await operation();
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
    loading,
    opLabel,
    statusText,
    errorText,
    searchQuery,
    lastRefreshAt,
    normalizedSearch,
    lastRefreshText,
    statusLine,
    setStatus,
    setError,
    runOperation,
    dispose
  };
});
