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

  watch(
    () => app.serviceUrl,
    (url) => {
      client.disconnect();

      if (url) {
        client.connect();
      }
    },
    { immediate: true },
  );

  watch(
    () => auth.accessTokenRevision,
    () => {
      client.disconnect();

      if (app.serviceUrl) {
        client.connect();
      }
    },
  );

  onBeforeUnmount(() => { client.disconnect(); });
}
