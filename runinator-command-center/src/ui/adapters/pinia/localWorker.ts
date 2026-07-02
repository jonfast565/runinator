import { defineStore } from "pinia";
import { computed } from "vue";
import { localWorkerService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useLocalWorkerStore = defineStore("localWorker", () => {
  const state = mirrorServiceState(localWorkerService);

  return {
    supported: localWorkerService.supported,
    status: computed(() => state.value.status),
    busy: computed(() => state.value.busy),
    error: computed(() => state.value.error),
    refresh: () => localWorkerService.refresh(),
    start: (config: Parameters<typeof localWorkerService.start>[0]) => localWorkerService.start(config),
    stop: () => localWorkerService.stop(),
  };
});
