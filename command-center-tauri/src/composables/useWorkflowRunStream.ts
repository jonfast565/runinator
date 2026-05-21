import { onBeforeUnmount, watch } from "vue";
import { useAppStore } from "../stores/app";
import { useWorkflowsStore } from "../stores/workflows";
import type { WorkflowRunDetail } from "../types/models";
import { isTerminalWorkflowRunStatus } from "../utils/status";
import { buildWebSocketUrl } from "../utils/websocket";

const RECONNECT_DELAY = 3000;

export function useWorkflowRunStream() {
  const workflows = useWorkflowsStore();
  const app = useAppStore();
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectionId = 0;
  const terminalRunIds = new Set<number>();

  function clearReconnectTimer() {
    if (reconnectTimer === null) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function connect(runId: number) {
    clearReconnectTimer();
    if (workflows.selectedWorkflowRunId !== runId) return;
    if (!app.serviceUrl) return;
    const currentConnection = ++connectionId;
    ws = new WebSocket(buildWebSocketUrl(app.serviceUrl, `/ws/workflow-runs/${runId}`));
    ws.onopen = () => {
      if (currentConnection !== connectionId) return;
      clearReconnectTimer();
      console.info("[command-center] workflow run stream connected", { runId });
    };
    ws.onmessage = ({ data }) => {
      if (currentConnection !== connectionId) return;
      try {
        console.info("[command-center] workflow run stream message", { runId, data });
        const detail = JSON.parse(data) as WorkflowRunDetail;
        workflows.setWorkflowRunDetail(detail);
        if (isTerminalWorkflowRunStatus(detail.run.status)) {
          terminalRunIds.add(detail.run.id);
        }
      } catch (err) {
        console.info("[command-center] failed to parse workflow run stream message", { runId, data, err });
      }
    };
    ws.onerror = (event) => {
      if (currentConnection !== connectionId) return;
      console.info("[command-center] workflow run stream error", { runId, event });
      ws?.close();
    };
    ws.onclose = () => {
      if (currentConnection !== connectionId) return;
      console.info("[command-center] workflow run stream closed", { runId });
      ws = null;
      const selectedDetail = workflows.workflowRunDetail;
      const selectedRunIsTerminal =
        selectedDetail?.run.id === runId && isTerminalWorkflowRunStatus(selectedDetail.run.status);
      if (terminalRunIds.has(runId) || selectedRunIsTerminal) {
        return;
      }
      if (workflows.selectedWorkflowRunId === runId && app.serviceConnected) {
        reconnectTimer = setTimeout(() => connect(runId), RECONNECT_DELAY);
      }
    };
  }

  function disconnect() {
    connectionId += 1;
    clearReconnectTimer();
    ws?.close();
    ws = null;
  }

  watch(
    () => workflows.selectedWorkflowRunId,
    (id) => {
      disconnect();
      if (id > 0) connect(id);
    },
    { immediate: true }
  );

  onBeforeUnmount(disconnect);
}
