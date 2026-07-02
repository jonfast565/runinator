import { defineStore } from "pinia";
import { ref } from "vue";
import { fetchProviders as fetchProvidersApi } from "../api/commandCenterApi";
import { errorMessage } from "../utils/format";
import type { ProviderMetadata } from "../types/models";

export const useProvidersStore = defineStore("providers", () => {
  const providers = ref<ProviderMetadata[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  // drives the providers view selection; set by deep links such as the run-timeline quick action.
  const focusedProvider = ref("");
  const focusedAction = ref("");

  async function fetchProviders() {
    loading.value = true;
    error.value = null;

    try {
      const response = await fetchProvidersApi();
      providers.value = response
        .map(normalizeProvider)
        .filter((provider) => provider.name)
        .sort((left, right) => left.name.localeCompare(right.name));
    } catch (err) {
      error.value = errorMessage(err) || "Failed to fetch providers";
    } finally {
      loading.value = false;
    }
  }

  // select a provider/action so the providers view opens focused on it.
  function focusProviderAction(provider: string, action = "") {
    focusedProvider.value = provider;
    focusedAction.value = action;
  }

  function clearProviders() {
    providers.value = [];
    error.value = null;
    loading.value = false;
    focusedProvider.value = "";
    focusedAction.value = "";
  }

  return {
    providers,
    loading,
    error,
    focusedProvider,
    focusedAction,
    fetchProviders,
    focusProviderAction,
    clearProviders,
  };
});

function normalizeProvider(
  provider: ProviderMetadata & { provider_name?: string },
): ProviderMetadata {
  return {
    name: provider.name || (provider.provider_name ?? ""),
    actions: [...provider.actions]
      .map((action) => ({
        ...action,
        parameters: action.parameters,
        results: action.results,
      }))
      .sort((left, right) => left.function_name.localeCompare(right.function_name)),
    metadata: {
      credential_scopes: provider.metadata.credential_scopes,
      contract: provider.metadata.contract ?? null,
    },
  };
}
