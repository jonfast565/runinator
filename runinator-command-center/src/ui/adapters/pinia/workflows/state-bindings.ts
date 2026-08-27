import { computed, type Ref } from "vue";
import type { WorkflowServices } from "../../../../core/services";

type WorkflowState = ReturnType<WorkflowServices["getState"]>;

/** reactive projections of mutable workflow service state. */
export function createWorkflowStateBindings(
  services: WorkflowServices,
  state: Ref<WorkflowState>,
) {
  return {
    workflows: computed({
      get: () => state.value.workflows,
      set: (workflows) => { services.setState((current) => ({ ...current, workflows })); },
    }),
    selectedWorkflowId: computed({
      get: () => state.value.selectedWorkflowId,
      set: (selectedWorkflowId) =>
        { services.setState((current) => ({ ...current, selectedWorkflowId })); },
    }),
    workflowJson: computed({
      get: () => state.value.workflowJson,
      set: (workflowJson) => { services.setState((current) => ({ ...current, workflowJson })); },
    }),
    workflowRexRap: computed({
      get: () => state.value.workflowRexRap,
      set: (workflowRexRap) => { services.setState((current) => ({ ...current, workflowRexRap })); },
    }),
    workflowRexRapError: computed(() => state.value.workflowRexRapError),
    headerDraft: computed(() => state.value.headerDraft),
    workflowSettingsOpen: computed({
      get: () => state.value.workflowSettingsOpen,
      set: (workflowSettingsOpen) =>
        { services.setState((current) => ({ ...current, workflowSettingsOpen })); },
    }),
    workflowTriggers: computed({
      get: () => state.value.workflowTriggers,
      set: (workflowTriggers) =>
        { services.setState((current) => ({ ...current, workflowTriggers })); },
    }),
    triggerEditorOpen: computed(() => state.value.triggerEditorOpen),
    triggerEditorCreating: computed(() => state.value.triggerEditorCreating),
    triggerEditorError: computed(() => state.value.triggerEditorError),
    workflowEditorMode: computed({
      get: () => state.value.workflowEditorMode,
      set: (workflowEditorMode) =>
        { services.setState((current) => ({ ...current, workflowEditorMode })); },
    }),
    workflowLayoutDirection: computed({
      get: () => state.value.workflowLayoutDirection,
      set: (workflowLayoutDirection) =>
        { services.setState((current) => ({ ...current, workflowLayoutDirection })); },
    }),
    workflowCanvasFocus: computed(() => state.value.workflowCanvasFocus),
    workflowInspectorMode: computed(() => state.value.workflowInspectorMode),
    stepEditorOpen: computed(() => state.value.stepEditorOpen),
    stepEditorCreating: computed(() => state.value.stepEditorCreating),
    stepEditorError: computed(() => state.value.stepEditorError),
    workflowRuns: computed({
      get: () => state.value.workflowRuns,
      set: (workflowRuns) => { services.setState((current) => ({ ...current, workflowRuns })); },
    }),
    workflowLayoutVersion: computed(() => state.value.workflowLayoutVersion),
    selectedWorkflowRunId: computed({
      get: () => state.value.selectedWorkflowRunId,
      set: (selectedWorkflowRunId) =>
        { services.setState((current) => ({ ...current, selectedWorkflowRunId })); },
    }),
    workflowRunDetail: computed(() => state.value.workflowRunDetail),
    workflowRunGates: computed(() => state.value.workflowRunGates),
    workflowNodeDetailExtra: computed(() => state.value.workflowNodeDetailExtra),
    selectedStepId: computed({
      get: () => state.value.selectedStepId,
      set: (selectedStepId) => { services.setState((current) => ({ ...current, selectedStepId })); },
    }),
    inlineEditNodeId: computed({
      get: () => state.value.inlineEditNodeId,
      set: (inlineEditNodeId) =>
        { services.setState((current) => ({ ...current, inlineEditNodeId })); },
    }),
    selectedWorkflowRunNodeId: computed({
      get: () => state.value.selectedWorkflowRunNodeId,
      set: (selectedWorkflowRunNodeId) =>
        { services.setState((current) => ({ ...current, selectedWorkflowRunNodeId })); },
    }),
    selectedWorkflowNodeRunId: computed({
      get: () => state.value.selectedWorkflowNodeRunId,
      set: (selectedWorkflowNodeRunId) =>
        { services.setState((current) => ({ ...current, selectedWorkflowNodeRunId })); },
    }),
    runInputOpen: computed(() => state.value.runInputOpen),
    runInputDraft: computed({
      get: () => state.value.runInputDraft,
      set: (runInputDraft) => { services.setState((current) => ({ ...current, runInputDraft })); },
    }),
    runInputDebug: computed(() => state.value.runInputDebug),
    selectedGraphEdgeId: computed({
      get: () => state.value.selectedGraphEdgeId,
      set: (selectedGraphEdgeId) =>
        { services.setState((current) => ({ ...current, selectedGraphEdgeId })); },
    }),
    openRunIds: computed(() => state.value.openRunIds),
    isDirty: computed(() => state.value.isDirty),
    headerIssueCount: computed(() => {
      void state.value.headerDraft;
      void state.value.workflowLayoutVersion;
      return services.header.getHeaderIssueCount();
    }),
    // the two panel badges, each counting only what its own tab can fix.
    interruptIssueCount: computed(() => {
      void state.value.headerDraft;
      void state.value.workflowLayoutVersion;
      return services.header.getInterruptIssueCount();
    }),
    declarationIssueCount: computed(() => {
      void state.value.headerDraft;
      void state.value.workflowLayoutVersion;
      return services.header.getDeclarationIssueCount();
    }),
    canRequestRunInterrupt: computed(() => {
      void state.value.workflowRunDetail;
      return services.canRequestRunInterrupt();
    }),
    requestableInterruptSources: computed(() => {
      void state.value.workflowRunDetail;
      return services.getRequestableInterruptSources();
    }),
  };
}
