import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { useAppStore } from "../stores/app";
import type { RunChunk } from "../types/models";
import { buildWebSocketUrl } from "../utils/websocket";

const RECONNECT_DELAY = 3000;

export function useRunLogStream(runId: Ref<string | null>) {
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

  function connect(id: string) {
    clearReconnectTimer();
    if (runId.value !== id) return;
    if (!app.serviceUrl) return;
    ws = new WebSocket(buildWebSocketUrl(app.serviceUrl, `/ws/run-stream/${id}`));
    ws.onopen = () => {
      clearReconnectTimer();
      console.info("[command-center] run log stream connected", { runId: id });
    };
    ws.onmessage = ({ data }) => {
      try {
        console.info("[command-center] run log stream message", { runId: id, data });
        chunks.value.push(JSON.parse(data) as RunChunk);
        lastChunkAt.value = Date.now();
      } catch (err) {
        console.info("[command-center] failed to parse run log stream message", { runId: id, data, err });
      }
    };
    ws.onerror = (event) => {
      console.info("[command-center] run log stream error", { runId: id, event });
      ws?.close();
    };
    ws.onclose = () => {
      console.info("[command-center] run log stream closed", { runId: id });
      ws = null;
      if (runId.value === id && app.serviceConnected) {
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
    runId,
    (id) => {
      disconnect();
      chunks.value = [];
      lastChunkAt.value = 0;
      if (id) connect(id);
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    disconnect();
  });

  return { chunks, lastChunkAt };
}
