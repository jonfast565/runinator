import { onBeforeUnmount, watch } from "vue";
import { endpointForTab, isResourceTab, useAppStore } from "../stores/app";
import { useArtifactsStore } from "../stores/artifacts";
import { useNotificationsStore } from "../stores/notifications";
import { useResourcesStore } from "../stores/resources";
import { useTasksStore } from "../stores/tasks";
import { useWorkflowsStore } from "../stores/workflows";
import { buildWebSocketUrl } from "../utils/websocket";

const RECONNECT_DELAY = 3000;
const FALLBACK_INTERVAL = 30000;
const CONNECT_TIMEOUT = 5000;

export function useEventStream() {
  const app = useAppStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();
  const artifacts = useArtifactsStore();
  const notifications = useNotificationsStore();
  // Tasks store still referenced by the rest of the app; we no longer poll it here.
  void useTasksStore;

  function refreshResourcesIfActive() {
    if (!isResourceTab(app.activeTab)) return;
    const endpoint = endpointForTab(app.activeTab);
    if (endpoint) void resources.refreshResourcesFor(endpoint);
  }
  let ws: WebSocket | null = null;
  let fallbackTimer: number | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectionId = 0;

  function handleEvent(event: { type: string; [k: string]: unknown }) {
    console.info("[command-center] server event", event);
    switch (event.type) {
      case "run_status_changed":
        if (workflows.selectedWorkflowRunId) workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
        refreshResourcesIfActive();
        break;
      case "resync":
        refreshActiveState();
        break;
      case "tasks_changed":
        break;
      case "workflows_changed":
        if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
        break;
      case "workflow_run_changed": {
        const runId = event.run_id as string;
        if (workflows.selectedWorkflowRunId === runId) {
          workflows.fetchWorkflowRunDetail(runId, true);
        }
        if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
        refreshResourcesIfActive();
        break;
      }
      case "resources_changed":
        refreshResourcesIfActive();
        break;
      case "artifact_created":
      case "artifacts_changed":
        if (app.activeTab === "Artifacts") void artifacts.refreshArtifacts();
        break;
      case "notification_created":
      case "notifications_changed":
        // Always refresh notifications so the sidebar badge can stay current.
        void notifications.refreshNotifications();
        break;
    }
  }

  function startFallback() {
    if (fallbackTimer !== null) return;
    app.setEventStreamState("fallback");
    fallbackTimer = window.setInterval(refreshActiveState, FALLBACK_INTERVAL);
  }

  function refreshActiveState() {
    if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
    if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
    if (workflows.selectedWorkflowRunId) {
      workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
    }
    if (app.activeTab === "Artifacts") void artifacts.refreshArtifacts();
    if (app.activeTab === "Notifications") void notifications.refreshNotifications();
    refreshResourcesIfActive();
  }

  function stopFallback() {
    if (fallbackTimer !== null) {
      clearInterval(fallbackTimer);
      fallbackTimer = null;
    }
  }

  function clearReconnectTimer() {
    if (reconnectTimer === null) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function clearConnectTimer() {
    if (connectTimer === null) return;
    clearTimeout(connectTimer);
    connectTimer = null;
  }

  function connect() {
    clearReconnectTimer();
    clearConnectTimer();
    const serviceUrl = app.serviceUrl;
    if (!serviceUrl) {
      startFallback();
      return;
    }
    const currentConnection = ++connectionId;
    app.setEventStreamState("connecting");
    const url = buildWebSocketUrl(serviceUrl, "/ws/events");
    ws = new WebSocket(url);
    connectTimer = setTimeout(() => {
      if (currentConnection !== connectionId) return;
      console.info("[command-center] event stream connection timed out", { url });
      ws?.close();
      startFallback();
    }, CONNECT_TIMEOUT);
    ws.onopen = () => {
      if (currentConnection !== connectionId) return;
      clearConnectTimer();
      console.info("[command-center] event stream connected", { url });
      app.setEventStreamState("connected");
      stopFallback();
    };
    ws.onmessage = ({ data }) => {
      if (currentConnection !== connectionId) return;
      try {
        console.info("[command-center] event stream message", data);
        handleEvent(JSON.parse(data));
      } catch (err) {
        console.info("[command-center] failed to parse event stream message", { data, err });
      }
    };
    ws.onclose = () => {
      if (currentConnection !== connectionId) return;
      clearConnectTimer();
      console.info("[command-center] event stream closed, starting fallback and scheduling reconnect", { url });
      ws = null;
      startFallback();
      if (app.serviceConnected) {
        reconnectTimer = setTimeout(connect, RECONNECT_DELAY);
      }
    };
    ws.onerror = (event) => {
      if (currentConnection !== connectionId) return;
      clearConnectTimer();
      console.info("[command-center] event stream connection error", { url, event });
      ws?.close();
    };
  }

  function disconnect() {
    connectionId += 1;
    clearReconnectTimer();
    clearConnectTimer();
    ws?.close();
    ws = null;
    stopFallback();
    app.setEventStreamState("disconnected");
  }

  watch(
    () => app.serviceUrl,
    (url) => {
      disconnect();
      if (url) connect();
      else startFallback();
    },
    { immediate: true }
  );

  onBeforeUnmount(disconnect);
}
