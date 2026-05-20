import { onBeforeUnmount, onMounted, ref } from "vue";
import { fetchSupervisorStatus, type SupervisorStatus } from "../api/commandCenterApi";

const POLL_INTERVAL_MS = 5000;

export function useSupervisorStatus() {
  const status = ref<SupervisorStatus | null>(null);
  const error = ref<string>("");
  let timer: number | undefined;

  async function refresh() {
    try {
      const next = await fetchSupervisorStatus();
      status.value = next;
      error.value = next.error ?? "";
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  onMounted(() => {
    refresh();
    timer = window.setInterval(refresh, POLL_INTERVAL_MS);
  });

  onBeforeUnmount(() => {
    if (timer !== undefined) window.clearInterval(timer);
  });

  return { status, error, refresh };
}
