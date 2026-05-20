import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { useAppStore } from "../stores/app";
import type { RunChunk } from "../types/models";

export function useWorkflowNodeRunLogStream(nodeRunId: Ref<number>) {
  const app = useAppStore();
  const chunks = ref<RunChunk[]>([]);
  const lastChunkAt = ref<number>(0);
  let ws: WebSocket | null = null;

  function connect(id: number) {
    chunks.value = [];
    lastChunkAt.value = 0;
    const base = app.serviceUrl?.replace(/^http/, "ws");
    if (!base) return;
    ws = new WebSocket(`${base}/ws/workflow-node-runs/${id}/stream`);
    ws.onopen = () => console.info("[command-center] workflow node run log stream connected", { nodeRunId: id });
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
    };
  }

  watch(
    nodeRunId,
    (id) => {
      ws?.close();
      ws = null;
      if (id > 0) connect(id);
      else chunks.value = [];
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    ws?.close();
  });

  return { chunks, lastChunkAt };
}
