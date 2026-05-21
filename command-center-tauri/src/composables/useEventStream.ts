import { onBeforeUnmount, watch } from "vue";
import { useAppStore } from "../stores/app";
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
  const tasks = useTasksStore();
  let ws: WebSocket | null = null;
  let fallbackTimer: number | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectionId = 0;

  function handleEvent(event: { type: string; [k: string]: unknown }) {
    console.info("[command-center] server event", event);
    switch (event.type) {
      case "run_status_changed":
        if (workflows.selectedWorkflowRunId > 0) workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
        if (app.activeTab === "Tasks") tasks.refreshRunsForSelectedTask();
        break;
      case "resync":
        refreshActiveState();
        break;
      case "tasks_changed":
        if (app.activeTab === "Tasks") tasks.refreshRunsForSelectedTask();
        break;
      case "workflows_changed":
        if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
        break;
      case "workflow_run_changed": {
        const runId = event.run_id as number;
        if (workflows.selectedWorkflowRunId === runId) {
          workflows.fetchWorkflowRunDetail(runId, true);
        }
        if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
        break;
      }
      case "resources_changed":
        if (app.activeTab === "Resources") resources.refreshResources();
        break;
    }
  }

  function startFallback() {
    if (fallbackTimer !== null) return;
    app.setEventStreamState("fallback");
    fallbackTimer = window.setInterval(() => {
      if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
      if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
      if (workflows.selectedWorkflowRunId > 0) {
        workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
      }
      if (app.activeTab === "Resources") resources.refreshResources();
      if (app.activeTab === "Tasks") tasks.refreshRunsForSelectedTask();
    }, FALLBACK_INTERVAL);
  }

  function refreshActiveState() {
    if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
    if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
    if (workflows.selectedWorkflowRunId > 0) {
      workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
    }
    if (app.activeTab === "Resources") resources.refreshResources();
    if (app.activeTab === "Tasks") tasks.refreshRunsForSelectedTask();
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
      console.info("[command-center] event stream closed");
      ws = null;
      startFallback();
      if (app.serviceConnected) {
        reconnectTimer = setTimeout(connect, RECONNECT_DELAY);
      }
    };
    ws.onerror = (event) => {
      if (currentConnection !== connectionId) return;
      clearConnectTimer();
      console.info("[command-center] event stream error", event);
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
