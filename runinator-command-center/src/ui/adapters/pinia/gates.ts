import { defineStore } from "pinia";
import { computed } from "vue";
import { gatesService, appService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useGatesStore = defineStore("gates", () => {
  const state = mirrorServiceState(gatesService);

  const filteredGates = computed(() => gatesService.filteredGates(appService.normalizedSearch));

  return {
    gates: computed(() => state.value.gates),
    selectedGate: computed({
      get: () => state.value.selectedGate,
      set: (gate) => { gatesService.setSelectedGate(gate); },
    }),
    filteredGates,
    canResolveSelected: computed(() => gatesService.canResolveSelected()),
    refreshGates: () => gatesService.refreshGates(),
    clearGates: () => { gatesService.clearGates(); },
    resolveSelected: (action: "open" | "close", reason?: string) =>
      gatesService.resolveSelected(action, reason),
    removeSelected: () =>
      gatesService.removeSelected({
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
  };
});
