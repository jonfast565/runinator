import { defineStore } from 'pinia'
import { ref } from 'vue'
import { fetchProviders as fetchProvidersApi } from '../api/commandCenterApi'
import type { ProviderMetadata } from '../types/models'

export const useProvidersStore = defineStore('providers', () => {
  const providers = ref<ProviderMetadata[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function fetchProviders() {
    loading.value = true
    error.value = null
    try {
      const response = await fetchProvidersApi()
      providers.value = response
        .map(normalizeProvider)
        .filter((provider) => provider.name)
        .sort((left, right) => left.name.localeCompare(right.name))
    } catch (err: any) {
      error.value = err.message || 'Failed to fetch providers'
    } finally {
      loading.value = false
    }
  }

  return {
    providers,
    loading,
    error,
    fetchProviders
  }
})

function normalizeProvider(provider: ProviderMetadata & { provider_name?: string }): ProviderMetadata {
  return {
    name: provider.name || provider.provider_name || '',
    actions: [...(provider.actions ?? [])]
      .map(action => ({
        ...action,
        parameters: action.parameters ?? [],
        results: action.results ?? []
      }))
      .sort((left, right) => left.function_name.localeCompare(right.function_name)),
    metadata: {
      credential_scopes: provider.metadata?.credential_scopes ?? [],
      contract: provider.metadata?.contract ?? null
    }
  }
}
