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
    ws.onopen = () => console.info("[command-center] workflow run stream connected", { runId });
    ws.onmessage = ({ data }) => {
      try {
        console.info("[command-center] workflow run stream message", { runId, data });
        workflows.setWorkflowRunDetail(JSON.parse(data) as WorkflowRunDetail);
      } catch (err) {
        console.info("[command-center] failed to parse workflow run stream message", { runId, data, err });
      }
    };
    ws.onerror = (event) => {
      console.info("[command-center] workflow run stream error", { runId, event });
      ws?.close();
    };
    ws.onclose = () => {
      console.info("[command-center] workflow run stream closed", { runId });
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
