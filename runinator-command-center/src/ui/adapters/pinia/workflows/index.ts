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

const WORKFLOW_REXRAP_SYNC_DELAY_MS = 1500;

function providerCatalog(): ProviderMetadata[] {
  return useProvidersStore().providers;
}

export const useWorkflowsStore = defineStore("workflows", () => {
  const services = workflowServices;
  const state = mirrorServiceState(services);
  const catalogState = mirrorServiceState(catalogMetadataService);
  const app = useAppStore();
  // these four are edited from both sides: a component writes a form field, and a service writes a
  // freshly hydrated draft. installing the *proxies* back into the service state is what makes both
  // directions observable — a service assignment like `host.state.triggerJson.configuration = ...`
  // reaches the raw object either way, but only a write through the proxy notifies a watcher, and
  // re-copying the object onto itself afterwards notifies nothing because nothing changed.
  const workflowDraft = reactive(services.getState().workflowDraft);
  const triggerDraft = reactive(services.getState().triggerDraft);
  const triggerJson = reactive(services.getState().triggerJson);
  const stepEditor = reactive(services.getState().stepEditor);
  services.setState((current) => ({
    ...current,
    workflowDraft,
    triggerDraft,
    triggerJson,
    stepEditor,
  }));

  // the services mutate these four in place (`Object.assign(host.state.workflowDraft, ...)`), never
  // by replacing the object. a replacement would orphan the installed proxy — the state would point
  // at a plain object nothing observes — so fold it back in and re-install rather than letting the
  // editors go quietly dead. the guard keeps the re-install from recursing past one pass.
  let reinstalling = false;
  services.subscribe(() => {
    const next = services.getState();
    const drifted =
      next.workflowDraft !== workflowDraft ||
      next.triggerDraft !== triggerDraft ||
      next.triggerJson !== triggerJson ||
      next.stepEditor !== stepEditor;

    if (!drifted || reinstalling) {
      return;
    }

    Object.assign(workflowDraft, next.workflowDraft);
    Object.assign(triggerDraft, next.triggerDraft);
    Object.assign(triggerJson, next.triggerJson);
    Object.assign(stepEditor, next.stepEditor);
    reinstalling = true;
    services.setState((current) => ({
      ...current,
      workflowDraft,
      triggerDraft,
      triggerJson,
      stepEditor,
    }));
    reinstalling = false;
  });

  let workflowRexRapSyncTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleWorkflowRexRapSync() {
    if (workflowRexRapSyncTimer) {
      clearTimeout(workflowRexRapSyncTimer);
    }

    workflowRexRapSyncTimer = setTimeout(() => {
      workflowRexRapSyncTimer = null;
      void services.editor.syncWorkflowRexRap();
    }, WORKFLOW_REXRAP_SYNC_DELAY_MS);
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
    () => state.value.workflowRexRap,
    () => {
      if (services.internal.workflowRexRapWriteGuard || state.value.workflowRexRapError) {
        return;
      }

      services.setState((current) => ({ ...current, workflowEditorMode: "rexrap" }));
      scheduleWorkflowRexRapSync();
    },
  );

  watch(
    stepEditor,
    () => {
      // hydrating the editor from a node is not an edit of it. without this the act of opening a
      // step would apply the freshly-read draft straight back over the node it came from, which is
      // how a half-populated field would overwrite a good one.
      if (state.value.stepEditorOpen && !services.internal.stepEditorHydrating) {
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
