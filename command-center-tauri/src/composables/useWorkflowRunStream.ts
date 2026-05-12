import { onBeforeUnmount, watch } from "vue";
import { useAppStore } from "../stores/app";
import { useWorkflowsStore } from "../stores/workflows";
import type { WorkflowRunDetail } from "../types/models";

export function useWorkflowRunStream() {
  const workflows = useWorkflowsStore();
  const app = useAppStore();
  let ws: WebSocket | null = null;

  function connect(runId: number) {
    const base = app.serviceUrl?.replace(/^http/, "ws");
    if (!base) return;
    ws = new WebSocket(`${base}/ws/workflow-runs/${runId}`);
    ws.onmessage = ({ data }) => {
      try {
        workflows.setWorkflowRunDetail(JSON.parse(data) as WorkflowRunDetail);
      } catch {}
    };
    ws.onerror = () => ws?.close();
    ws.onclose = () => {
      ws = null;
    };
  }

  function disconnect() {
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
