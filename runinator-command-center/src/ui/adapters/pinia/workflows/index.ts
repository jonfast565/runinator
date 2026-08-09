import { defineStore } from "pinia";
import { reactive, watch } from "vue";
import type { ProviderMetadata } from "../../../../core/domain/models";
import { catalogMetadataService, workflowServices } from "../../../../core/services";
import { useAppStore } from "../app";
import { useProvidersStore } from "../providers";
import { mirrorServiceState } from "../sync";
import { createWorkflowActions } from "./actions";
import { createWorkflowGraphHandlers } from "./graph-events";
import { createWorkflowSelectors } from "./selectors";
import { createWorkflowStateBindings } from "./state-bindings";

export {
  buildInputSkeleton,
  newWorkflowDraft,
  newWorkflowTriggerDraft,
} from "../../../../core/workflow/editor-defaults";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;

function providerCatalog(): ProviderMetadata[] {
  return useProvidersStore().providers;
}

export const useWorkflowsStore = defineStore("workflows", () => {
  const services = workflowServices;
  const state = mirrorServiceState(services);
  const catalogState = mirrorServiceState(catalogMetadataService);
  const app = useAppStore();
  const workflowDraft = reactive(services.getState().workflowDraft);
  const triggerDraft = reactive(services.getState().triggerDraft);
  const triggerJson = reactive(services.getState().triggerJson);
  const stepEditor = reactive(services.getState().stepEditor);

  services.subscribe(() => {
    const next = services.getState();
    Object.assign(workflowDraft, next.workflowDraft);
    Object.assign(triggerDraft, next.triggerDraft);
    Object.assign(triggerJson, next.triggerJson);
    Object.assign(stepEditor, next.stepEditor);
  });

  let workflowWdlSyncTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleWorkflowWdlSync() {
    if (workflowWdlSyncTimer) {
      clearTimeout(workflowWdlSyncTimer);
    }

    workflowWdlSyncTimer = setTimeout(() => {
      workflowWdlSyncTimer = null;
      void services.editor.syncWorkflowWdl();
    }, WORKFLOW_WDL_SYNC_DELAY_MS);
  }

  watch(
    () => state.value.workflowJson,
    () => {
      if (services.internal.workflowJsonWriteGuard || state.value.workflowEditorMode !== "json") {
        return;
      }

      void services.editor.syncWorkflowJson();
    },
  );

  watch(
    () => state.value.workflowWdl,
    () => {
      if (services.internal.workflowWdlWriteGuard || state.value.workflowWdlError) {
        return;
      }

      services.setState((current) => ({ ...current, workflowEditorMode: "wdl" }));
      scheduleWorkflowWdlSync();
    },
  );

  watch(
    stepEditor,
    () => {
      if (state.value.stepEditorOpen) {
        services.editor.scheduleStepEditorApply();
      }
    },
    { deep: true },
  );

  const selectors = createWorkflowSelectors({
    services,
    state,
    catalogState,
    workflowDraft,
    searchQuery: () => app.normalizedSearch,
    providerCatalog,
  });

  return {
    workflowDraft,
    triggerDraft,
    triggerJson,
    stepEditor,
    ...selectors,
    ...createWorkflowStateBindings(services, state),
    ...createWorkflowGraphHandlers(services),
    ...createWorkflowActions(services),
  };
});
