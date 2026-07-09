import { onBeforeUnmount, watch } from "vue";
import { endpointForTab, isResourceTab, useAppStore } from "../../ui/adapters/pinia/app";
import { useArtifactsStore } from "../../ui/adapters/pinia/artifacts";
import { useAuthStore } from "../../ui/adapters/pinia/auth";
import { useNotificationsStore } from "../../ui/adapters/pinia/notifications";
import { useResourcesStore } from "../../ui/adapters/pinia/resources";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { createEventStreamRouter } from "../../core/realtime/event-router";
import { EventStreamClient } from "../../core/realtime/event-stream-client";

export function useEventStream() {
  const app = useAppStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();
  const artifacts = useArtifactsStore();
  const notifications = useNotificationsStore();
  const auth = useAuthStore();

  function refreshResourcesIfActive() {
    if (!isResourceTab(app.activeTab)) {
      return;
    }

    const endpoint = endpointForTab(app.activeTab);

    if (endpoint) {
      void resources.refreshResourcesFor(endpoint);
    }
  }

  function refreshActiveState() {
    if (app.activeTab === "Workflows" && !workflows.isDirty) {
      void workflows.refreshWorkflows();
    }

    if (app.activeTab === "Runs") {
      void workflows.fetchRecentWorkflowRuns();
    }

    if (workflows.selectedWorkflowRunId) {
      void workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
    }

    if (app.activeTab === "Artifacts") {
      void artifacts.refreshArtifacts();
    }

    if (app.activeTab === "Notifications") {
      void notifications.refreshNotifications();
    }

    refreshResourcesIfActive();
  }

  const router = createEventStreamRouter(() => ({
    activeTab: app.activeTab,
    selectedWorkflowRunId: workflows.selectedWorkflowRunId,
    isWorkflowEditorDirty: workflows.isDirty,
    refreshResourcesIfActive,
    refreshActiveState,
    refreshWorkflowsIfClean: () => {
      if (app.activeTab === "Workflows" && !workflows.isDirty) {
        void workflows.refreshWorkflows();
      }
    },
    refreshRecentRunsIfActive: () => {
      if (app.activeTab === "Runs") {
        void workflows.fetchRecentWorkflowRuns();
      }
    },
    refreshWorkflowRunIfSelected: (runId: string) => {
      void workflows.fetchWorkflowRunDetail(runId, true);
    },
    refreshArtifactsIfActive: () => {
      if (app.activeTab === "Artifacts") {
        void artifacts.refreshArtifacts();
      }
    },
    refreshNotifications: () => {
      void notifications.refreshNotifications();
    },
  }));

  const client = new EventStreamClient({
    getServiceUrl: () => app.serviceUrl,
    getServiceKnown: () => app.serviceKnown,
    onStateChange: (state) => { app.setEventStreamState(state); },
    onFallbackTick: refreshActiveState,
    router,
  });

  function shouldConnect(): boolean {
    // a browser WebSocket authenticates only via the ?token= query, so when auth is enabled don't
    // open a socket until we're authenticated — otherwise a logged-out or expired session flaps the
    // stream in an endless tokenless-401 reconnect loop. when auth is disabled `required` is false,
    // so this is always allowed.
    return Boolean(app.serviceUrl) && (!auth.required || auth.authenticated);
  }

  function reconnect() {
    client.disconnect();

    if (shouldConnect()) {
      client.connect();
    }
  }

  // reconnect on any input that changes whether/how we should be connected: the service url, auth
  // gating (required/authenticated), and the token revision (so a refreshed token is picked up).
  watch(
    [
      () => app.serviceUrl,
      () => auth.required,
      () => auth.authenticated,
      () => auth.accessTokenRevision,
    ],
    reconnect,
    { immediate: true },
  );

  onBeforeUnmount(() => { client.disconnect(); });
}
