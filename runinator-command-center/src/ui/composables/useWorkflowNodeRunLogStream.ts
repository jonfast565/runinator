import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { useAppStore } from "../../stores/app";
import { useAuthStore } from "../../stores/auth";
import type { RunChunk } from "../../types/models";
import { buildWebSocketUrl } from "../../utils/websocket";

const RECONNECT_DELAY = 3000;

export function useWorkflowNodeRunLogStream(nodeRunId: Ref<string | null>) {
  const app = useAppStore();
  const auth = useAuthStore();
  const chunks = ref<RunChunk[]>([]);
  const lastChunkAt = ref<number>(0);
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let connectionId = 0;

  function clearReconnectTimer() {
    if (reconnectTimer === null) {
      return;
    }

    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function connect(id: string) {
    clearReconnectTimer();

    if (nodeRunId.value !== id) {
      return;
    }

    if (!app.serviceUrl) {
      return;
    }

    const currentConnection = ++connectionId;
    ws = new WebSocket(buildWebSocketUrl(app.serviceUrl, `/ws/workflow-node-runs/${id}/stream`));

    ws.onopen = () => {
      if (currentConnection !== connectionId) {
        return;
      }

      clearReconnectTimer();
      console.info("[command-center] workflow node run log stream connected", { nodeRunId: id });
    };

    ws.onmessage = ({ data }: MessageEvent<string>) => {
      if (currentConnection !== connectionId) {
        return;
      }

      try {
        chunks.value.push(JSON.parse(data) as RunChunk);
        lastChunkAt.value = Date.now();
      } catch (err) {
        console.info("[command-center] failed to parse workflow node run log stream message", {
          nodeRunId: id,
          data,
          err,
        });
      }
    };

    ws.onerror = (event) => {
      if (currentConnection !== connectionId) {
        return;
      }

      console.info("[command-center] workflow node run log stream error", { nodeRunId: id, event });
      ws?.close();
    };

    ws.onclose = () => {
      if (currentConnection !== connectionId) {
        return;
      }

      console.info("[command-center] workflow node run log stream closed", { nodeRunId: id });
      ws = null;

      if (nodeRunId.value === id && app.serviceKnown) {
        reconnectTimer = setTimeout(() => {
          connect(id);
        }, RECONNECT_DELAY);
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
    nodeRunId,
    (id) => {
      disconnect();
      chunks.value = [];
      lastChunkAt.value = 0;

      if (id) {
        connect(id);
      }
    },
    { immediate: true },
  );

  watch(
    () => auth.accessTokenRevision,
    () => {
      const id = nodeRunId.value;
      disconnect();

      if (id) {
        connect(id);
      }
    },
  );

  onBeforeUnmount(() => {
    disconnect();
  });

  return { chunks, lastChunkAt };
}
