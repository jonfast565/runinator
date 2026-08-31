import { onBeforeUnmount, ref, watch } from "vue";
import { supervisorService, type SupervisorStatus } from "../../core/services";
import { useAuthStore } from "../adapters/pinia/auth";

const POLL_INTERVAL_MS = 5000;

export function useSupervisorStatus() {
  const auth = useAuthStore();
  const status = ref<SupervisorStatus | null>(null);
  const error = ref<string>("");
  let timer: number | undefined;

  async function refresh() {
    try {
      const next = await supervisorService.fetchStatus();
      // The supervisor only exists for a locally managed stack. Its endpoint deliberately returns
      // a 404 payload when it is absent, so probe it once and then stop generating console noise.

      if (!next.configured) {
        status.value = null;
        error.value = "";
        stopPolling();
        return;
      }

      status.value = next;
      error.value = next.error ?? "";
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  function stopPolling() {
    if (timer !== undefined) {
      window.clearInterval(timer);
      timer = undefined;
    }
  }

  function startPolling() {
    stopPolling();
    void refresh();
    timer = window.setInterval(refresh, POLL_INTERVAL_MS);
  }

  // Do not make an optional authenticated request while the shell is still resolving a restored
  // session. That initial tokenless request was the transient 401 seen in the browser console.
  watch(
    () => [auth.ready, auth.required, auth.authenticated] as const,
    ([ready, required, authenticated]) => {
      stopPolling();
      status.value = null;
      error.value = "";

      if (ready && (!required || authenticated)) {
        startPolling();
      }
    },
    { immediate: true },
  );

  onBeforeUnmount(() => {
    stopPolling();
  });

  return { status, error, refresh };
}
