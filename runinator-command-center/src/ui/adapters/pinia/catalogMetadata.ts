import { defineStore } from "pinia";
import { computed } from "vue";
import { catalogMetadataService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useCatalogMetadataStore = defineStore("catalogMetadata", () => {
  const state = mirrorServiceState(catalogMetadataService);

  return {
    nodeKinds: computed(() => state.value.nodeKinds),
    triggerKinds: computed(() => state.value.triggerKinds),
    enums: computed(() => state.value.enums),
    loading: computed(() => state.value.loading),
    loaded: computed(() => state.value.loaded),
    error: computed(() => state.value.error),
    fetchCatalogs: (force = false) => catalogMetadataService.fetchCatalogs(force),
    nodeKind: (kind: string) => catalogMetadataService.nodeKind(kind),
    triggerKind: (kind: string) => catalogMetadataService.triggerKind(kind),
    enumOptions: (name: string) => catalogMetadataService.enumOptions(name),
  };
});
