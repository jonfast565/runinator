import { defineStore } from "pinia";
import { computed } from "vue";
import { providersService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useProvidersStore = defineStore("providers", () => {
  const state = mirrorServiceState(providersService);

  return {
    providers: computed({
      get: () => state.value.providers,
      set: (providers) => {
        providersService.setState((current) => ({ ...current, providers }));
      },
    }),
    loading: computed(() => state.value.loading),
    error: computed(() => state.value.error),
    focusedProvider: computed(() => state.value.focusedProvider),
    focusedAction: computed(() => state.value.focusedAction),
    fetchProviders: () => providersService.fetchProviders(),
    focusProviderAction: (provider: string, action = "") =>
      providersService.focusProviderAction(provider, action),
    clearProviders: () => { providersService.clearProviders(); },
  };
});
