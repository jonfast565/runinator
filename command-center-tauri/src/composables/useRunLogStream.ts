import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { useAppStore } from "../stores/app";
import type { RunChunk } from "../types/models";

export function useRunLogStream(runId: Ref<number>) {
  const app = useAppStore();
  const chunks = ref<RunChunk[]>([]);
  let ws: WebSocket | null = null;

  function connect(id: number) {
    chunks.value = [];
    const base = app.serviceUrl?.replace(/^http/, "ws");
    if (!base) return;
    ws = new WebSocket(`${base}/ws/runs/${id}/stream`);
    ws.onmessage = ({ data }) => {
      try {
        chunks.value.push(JSON.parse(data) as RunChunk);
      } catch {}
    };
    ws.onerror = () => ws?.close();
    ws.onclose = () => {
      ws = null;
    };
  }

  watch(
    runId,
    (id) => {
      ws?.close();
      ws = null;
      if (id > 0) connect(id);
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    ws?.close();
  });

  return { chunks };
}
