import { fetchProviders as fetchProvidersApi } from "../api/commandCenterApi";
import type { ProviderMetadata } from "../domain/models";
import { errorMessage } from "../utils/format";
import { createStore } from "./event-bus";

export interface ProvidersState {
  providers: ProviderMetadata[];
  loading: boolean;
  error: string | null;
  focusedProvider: string;
  focusedAction: string;
}

export function createProvidersService() {
  const store = createStore<ProvidersState>({
    providers: [],
    loading: false,
    error: null,
    focusedProvider: "",
    focusedAction: "",
  });

  const service = {
    ...store,
    focusProviderAction(provider: string, action = "") {
      store.setState((state) => ({
        ...state,
        focusedProvider: provider,
        focusedAction: action,
      }));
    },
    async fetchProviders() {
      store.setState((state) => ({ ...state, loading: true, error: null }));

      try {
        const response = await fetchProvidersApi();
        const providers = response
          .map(normalizeProvider)
          .filter((provider) => provider.name)
          .sort((left, right) => left.name.localeCompare(right.name));
        store.setState((state) => ({ ...state, providers, loading: false }));
      } catch (err) {
        store.setState((state) => ({
          ...state,
          error: errorMessage(err) || "Failed to fetch providers",
          loading: false,
        }));
      }
    },
    clearProviders() {
      store.setState(() => ({
        providers: [],
        error: null,
        loading: false,
        focusedProvider: "",
        focusedAction: "",
      }));
    },
  };

  return service;
}

export type ProvidersService = ReturnType<typeof createProvidersService>;

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
