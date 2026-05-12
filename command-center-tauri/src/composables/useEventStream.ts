import { onBeforeUnmount, watch } from "vue";
import { useAppStore } from "../stores/app";
import { useResourcesStore } from "../stores/resources";
import { useTasksStore } from "../stores/tasks";
import { useWorkflowsStore } from "../stores/workflows";

const RECONNECT_DELAY = 3000;
const FALLBACK_INTERVAL = 30000;

export function useEventStream() {
  const app = useAppStore();
  const tasks = useTasksStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();
  let ws: WebSocket | null = null;
  let fallbackTimer: number | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  function handleEvent(event: { type: string; [k: string]: unknown }) {
    switch (event.type) {
      case "tasks_changed":
        if (!tasks.taskEditorOpen) tasks.refreshTasks();
        break;
      case "run_status_changed":
        tasks.refreshRunsForSelectedTask();
        break;
      case "workflows_changed":
        if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
        break;
      case "workflow_run_changed": {
        const runId = event.run_id as number;
        if (workflows.selectedWorkflowRunId === runId) {
          workflows.fetchWorkflowRunDetail(runId, true);
        }
        break;
      }
      case "workflow_run_activity":
        if (workflows.selectedWorkflowRunId > 0) {
          workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
        }
        break;
      case "resources_changed":
        if (app.activeTab === "Resources") resources.refreshResources();
        break;
    }
  }

  function startFallback() {
    if (fallbackTimer !== null) return;
    fallbackTimer = window.setInterval(() => {
      if (!tasks.taskEditorOpen) {
        tasks.refreshTasks();
        if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
        if (workflows.selectedWorkflowRunId > 0) {
          workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId, true);
        }
        if (app.activeTab === "Resources") resources.refreshResources();
      }
    }, FALLBACK_INTERVAL);
  }

  function stopFallback() {
    if (fallbackTimer !== null) {
      clearInterval(fallbackTimer);
      fallbackTimer = null;
    }
  }

  function connect() {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    const base = app.serviceUrl?.replace(/^http/, "ws");
    if (!base) {
      startFallback();
      return;
    }
    ws = new WebSocket(`${base}/ws/events`);
    ws.onopen = () => stopFallback();
    ws.onmessage = ({ data }) => {
      try {
        handleEvent(JSON.parse(data));
      } catch {}
    };
    ws.onclose = () => {
      ws = null;
      startFallback();
      if (app.serviceConnected) {
        reconnectTimer = setTimeout(connect, RECONNECT_DELAY);
      }
    };
    ws.onerror = () => ws?.close();
  }

  function disconnect() {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    ws?.close();
    ws = null;
    stopFallback();
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
