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
    // read the reactive mirror (not the service's getState) so lookups called inside a component
    // computed establish a dependency and re-run when the catalog finishes loading.
    nodeKind: (kind: string) => state.value.nodeKinds.find((entry) => entry.kind === kind),
    triggerKind: (kind: string) => state.value.triggerKinds.find((entry) => entry.kind === kind),
    enumOptions: (name: string) =>
      state.value.enums.find((entry) => entry.name === name)?.options ?? [],
  };
});
