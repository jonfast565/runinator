import {
  fetchEnumCatalogs as fetchEnumCatalogsApi,
  fetchNodeKinds as fetchNodeKindsApi,
  fetchTriggerKinds as fetchTriggerKindsApi,
} from "../api/commandCenterApi";
import type {
  EnumCatalogMetadata,
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
} from "../domain/models";
import { setWorkflowCatalogs } from "../workflow/catalog-registry";
import { errorMessage } from "../utils/format";
import { createStore } from "./event-bus";

export interface CatalogMetadataState {
  nodeKinds: WorkflowNodeKindMetadata[];
  triggerKinds: WorkflowTriggerKindMetadata[];
  enums: EnumCatalogMetadata[];
  loading: boolean;
  loaded: boolean;
  error: string | null;
}

// the node/edge/trigger metadata catalogs are compile-time constants on the backend, so they are
// fetched once and cached for the session. the workflow editor, palette, detail view, and trigger
// forms all render generically from this state instead of hardcoding per-kind knowledge.
export function createCatalogMetadataService() {
  const store = createStore<CatalogMetadataState>({
    nodeKinds: [],
    triggerKinds: [],
    enums: [],
    loading: false,
    loaded: false,
    error: null,
  });

  const service = {
    ...store,
    async fetchCatalogs(force = false) {
      const current = store.getState();

      if (current.loaded && !force) {
        return;
      }

      store.setState((state) => ({ ...state, loading: true, error: null }));

      try {
        const [nodeKinds, triggerKinds, enums] = await Promise.all([
          fetchNodeKindsApi(),
          fetchTriggerKindsApi(),
          fetchEnumCatalogsApi(),
        ]);
        setWorkflowCatalogs({ nodeKinds, triggerKinds, enums });
        store.setState((state) => ({
          ...state,
          nodeKinds,
          triggerKinds,
          enums,
          loading: false,
          loaded: true,
        }));
      } catch (err) {
        store.setState((state) => ({
          ...state,
          error: errorMessage(err) || "Failed to fetch workflow catalog metadata",
          loading: false,
        }));
      }
    },
    nodeKind(kind: string): WorkflowNodeKindMetadata | undefined {
      return store.getState().nodeKinds.find((entry) => entry.kind === kind);
    },
    triggerKind(kind: string): WorkflowTriggerKindMetadata | undefined {
      return store.getState().triggerKinds.find((entry) => entry.kind === kind);
    },
    enumOptions(name: string) {
      return store.getState().enums.find((entry) => entry.name === name)?.options ?? [];
    },
  };

  return service;
}

export type CatalogMetadataService = ReturnType<typeof createCatalogMetadataService>;
