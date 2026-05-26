import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { useAppStore } from "../stores/app";
import type { RunChunk } from "../types/models";
import { buildWebSocketUrl } from "../utils/websocket";

const RECONNECT_DELAY = 3000;

export function useWorkflowNodeRunLogStream(nodeRunId: Ref<number>) {
  const app = useAppStore();
  const chunks = ref<RunChunk[]>([]);
  const lastChunkAt = ref<number>(0);
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  function clearReconnectTimer() {
    if (reconnectTimer === null) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function connect(id: number) {
    clearReconnectTimer();
    if (nodeRunId.value !== id) return;
    if (!app.serviceUrl) return;
    ws = new WebSocket(buildWebSocketUrl(app.serviceUrl, `/ws/workflow-node-runs/${id}/stream`));
    ws.onopen = () => {
      clearReconnectTimer();
      console.info("[command-center] workflow node run log stream connected", { nodeRunId: id });
    };
    ws.onmessage = ({ data }) => {
      try {
        chunks.value.push(JSON.parse(data) as RunChunk);
        lastChunkAt.value = Date.now();
      } catch (err) {
        console.info("[command-center] failed to parse workflow node run log stream message", { nodeRunId: id, data, err });
      }
    };
    ws.onerror = (event) => {
      console.info("[command-center] workflow node run log stream error", { nodeRunId: id, event });
      ws?.close();
    };
    ws.onclose = () => {
      console.info("[command-center] workflow node run log stream closed", { nodeRunId: id });
      ws = null;
      if (nodeRunId.value === id && app.serviceConnected) {
        reconnectTimer = setTimeout(() => connect(id), RECONNECT_DELAY);
      }
    };
  }

  function disconnect() {
    clearReconnectTimer();
    ws?.close();
    ws = null;
  }

  watch(
    nodeRunId,
    (id) => {
      disconnect();
      chunks.value = [];
      lastChunkAt.value = 0;
      if (id > 0) connect(id);
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    disconnect();
  });

  return { chunks, lastChunkAt };
}
