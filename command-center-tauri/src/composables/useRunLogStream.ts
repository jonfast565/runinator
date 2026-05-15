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
    ws.onopen = () => console.info("[command-center] run log stream connected", { runId: id });
    ws.onmessage = ({ data }) => {
      try {
        console.info("[command-center] run log stream message", { runId: id, data });
        chunks.value.push(JSON.parse(data) as RunChunk);
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
