import { defineStore } from "pinia";
import { computed } from "vue";
import { authService } from "../../../core/services";
import { isTauriRuntime } from "../../../ui/adapters/tauri/runtime";
import {
  endpointForTab,
  isResourceTab,
  navItemForTab,
  navSections,
  tabs,
  visibleNavSections,
} from "../../../core/navigation/nav-config";
import { appService } from "../../../core/services";
import type { AppTab } from "../../../core/navigation/app";
import { mirrorServiceState } from "./sync";

export {
  endpointForTab,
  isResourceTab,
  navItemForTab,
  navSections,
  tabs,
  visibleNavSections,
};
export type { EventStreamState, ToastKind, Toast } from "../../../core/services/app";
export { isNetworkError } from "../../../core/services/app";
export type { AppTab } from "../../../core/navigation/app";

export const useAppStore = defineStore("app", () => {
  const state = mirrorServiceState(appService);

  const normalizedSearch = computed(() => appService.normalizedSearch);
  const lastRefreshText = computed(() =>
    state.value.lastRefreshAt ? state.value.lastRefreshAt.toLocaleTimeString() : "-",
  );
  const statusLine = computed(() => {
    if (state.value.errorText) {
      return `Error: ${state.value.errorText}`;
    }

    if (state.value.loading || state.value.opLabel) {
      return `${state.value.opLabel || "Working"}...`;
    }

    return state.value.statusText || "Ready.";
  });
  const serviceLabel = computed(
    () =>
      state.value.serviceUrl ??
      (state.value.backendReachable ? "Service reachable" : "No service discovered"),
  );
  const serviceKnown = computed(() => Boolean(state.value.serviceUrl));
  const serviceConnected = computed(() => state.value.backendReachable);
  const serviceBlocked = computed(
    () => state.value.initialLoading || (!state.value.errorText && !serviceKnown.value),
  );
  const outageActive = computed(
    () => serviceKnown.value && !state.value.backendReachable && !state.value.initialLoading,
  );
  const interactionsDisabled = computed(() => serviceBlocked.value || outageActive.value);
  const showOutageBanner = computed(() => outageActive.value && !state.value.outageDismissed);
  const loadingMessage = computed(() =>
    serviceConnected.value ? "Loading Runinator..." : "Waiting for Runinator service...",
  );
  const isRealtime = computed(() => state.value.eventStreamState === "connected");
  const eventStreamLabel = computed(() => {
    switch (state.value.eventStreamState) {
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
    () => state.value.replicas.filter((replica) => replica.status === "live").length,
  );
  const hasReplicaState = computed(() => state.value.replicas.length > 0);

  function visibleNav() {
    const auth = authService.getState();
    return visibleNavSections({
      canSeeAdmin: !auth.required || auth.user?.is_admin === true,
      isDesktop: isTauriRuntime(),
    });
  }

  return {
    activeTab: computed({
      get: () => state.value.activeTab,
      set: (tab: AppTab) => { appService.setActiveTab(tab); },
    }),
    sidebarCollapsed: computed({
      get: () => state.value.sidebarCollapsed,
      set: (value: boolean) => {
        if (value !== state.value.sidebarCollapsed) {
          appService.toggleSidebar();
        }
      },
    }),
    mobileNavOpen: computed(() => state.value.mobileNavOpen),
    serviceUrl: computed(() => state.value.serviceUrl),
    backendReachable: computed(() => state.value.backendReachable),
    initialLoading: computed({
      get: () => state.value.initialLoading,
      set: (value: boolean) => { appService.setInitialLoading(value); },
    }),
    loading: computed(() => state.value.loading),
    opLabel: computed(() => state.value.opLabel),
    statusText: computed(() => state.value.statusText),
    errorText: computed(() => state.value.errorText),
    searchQuery: computed({
      get: () => state.value.searchQuery,
      set: (value: string) => { appService.setSearchQuery(value); },
    }),
    lastRefreshAt: computed(() => state.value.lastRefreshAt),
    eventStreamState: computed(() => state.value.eventStreamState),
    replicaCounts: computed(() => state.value.replicaCounts),
    replicas: computed(() => state.value.replicas),
    toasts: computed(() => state.value.toasts),
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
    openMobileNav: () => { appService.openMobileNav(); },
    closeMobileNav: () => { appService.closeMobileNav(); },
    toggleMobileNav: () => { appService.toggleMobileNav(); },
    toggleSidebar: () => { appService.toggleSidebar(); },
    setStatus: (text: string) => { appService.setStatus(text); },
    setError: (text: string) => { appService.setError(text); },
    pushToast: appService.pushToast,
    dismissToast: appService.dismissToast,
    clearToasts: () => { appService.clearToasts(); },
    markBackendReachable: () => { appService.markBackendReachable(); },
    markBackendUnreachable: () => { appService.markBackendUnreachable(); },
    dismissOutageBanner: () => { appService.dismissOutageBanner(); },
    setServiceUrl: (url: string | null | undefined) => { appService.setServiceUrl(url); },
    setEventStreamState: appService.setEventStreamState,
    setReplicaState: appService.setReplicaState,
    clearReplicaState: () => { appService.clearReplicaState(); },
    refreshReplicas: () => appService.refreshReplicas(),
    runOperation: appService.runOperation.bind(appService),
    dispose: () => { appService.dispose(); },
    visibleNavSections: visibleNav,
  };
});
