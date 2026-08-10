import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type {
  Pipeline,
  PipelineDefaults,
  PipelineMemberFailureMode,
  WorkflowDefinition,
  WorkflowTrigger,
} from "../../../../core/domain/models";
import { defaultPipelineDefaults } from "../../../../core/domain/models";
import {
  createChainLink,
  deleteChainLink,
  deletePipeline as deletePipelineService,
  fetchPipelines,
  loadPipelineData,
  savePipeline,
  setPipelineOwner as setPipelineOwnerService,
  updateChainLink,
} from "../../../../core/services/pipeline";
import {
  buildPipelineGraph,
  type ChainEvent,
  type PipelineEdgeModel,
  type PipelineNodeModel,
  type UnresolvedChain,
} from "../../../../core/workflow/pipeline-graph";

// the pipeline canvas store. a pipeline is a persisted instance grouping member workflows; its
// links are `chained` triggers stamped with the pipeline id. state is derived: pipelines + the
// selected pipeline's members/triggers are loaded, then the graph is rebuilt from them.
export const usePipelineStore = defineStore("pipeline", () => {
  const pipelines = ref<Pipeline[]>([]);
  const selectedPipelineId = ref<string | null>(null);
  const allWorkflows = ref<WorkflowDefinition[]>([]);
  const triggersByWorkflowId = ref<Record<string, WorkflowTrigger[]>>({});
  const nodes = ref<PipelineNodeModel[]>([]);
  const edges = ref<PipelineEdgeModel[]>([]);
  const unresolved = ref<UnresolvedChain[]>([]);
  const selectedEdgeId = ref<string | null>(null);
  const selectedNodeId = ref<string | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const selectedPipeline = computed(
    () => pipelines.value.find((p) => p.id === selectedPipelineId.value) ?? null,
  );

  const memberWorkflows = computed(() => {
    const ids = new Set(selectedPipeline.value?.workflow_ids ?? []);
    return allWorkflows.value.filter(
      (wf): wf is WorkflowDefinition & { id: string } => wf.id != null && ids.has(wf.id),
    );
  });

  // workflows not yet in the selected pipeline, offered by the "add workflow" picker.
  const availableWorkflows = computed(() => {
    const ids = new Set(selectedPipeline.value?.workflow_ids ?? []);
    return allWorkflows.value.filter(
      (wf): wf is WorkflowDefinition & { id: string } => wf.id != null && !ids.has(wf.id),
    );
  });

  function rebuild() {
    const pipeline = selectedPipeline.value;

    if (!pipeline) {
      nodes.value = [];
      edges.value = [];
      unresolved.value = [];
      selectedEdgeId.value = null;
      selectedNodeId.value = null;
      return;
    }

    const graph = buildPipelineGraph(allWorkflows.value, triggersByWorkflowId.value, {
      pipelineId: pipeline.id,
      memberIds: pipeline.workflow_ids,
      memberFailureModes: pipeline.member_failure_modes,
      defaultFailureMode: pipeline.defaults.default_failure_mode,
    });
    nodes.value = graph.nodes;
    edges.value = graph.edges;
    unresolved.value = graph.unresolved;

    if (selectedEdgeId.value && !edges.value.some((edge) => edge.id === selectedEdgeId.value)) {
      selectedEdgeId.value = null;
    }

    if (selectedNodeId.value && !nodes.value.some((node) => node.id === selectedNodeId.value)) {
      selectedNodeId.value = null;
    }
  }

  // reload the selected pipeline's members + links and rebuild the graph.
  async function refreshGraph() {
    const pipeline = selectedPipeline.value;

    if (!pipeline) {
      allWorkflows.value = allWorkflows.value.length ? allWorkflows.value : [];
      rebuild();
      return;
    }

    const data = await loadPipelineData(pipeline.workflow_ids);
    allWorkflows.value = data.workflows;
    triggersByWorkflowId.value = data.triggersByWorkflowId;
    rebuild();
  }

  // reload the pipeline list; preserve selection when possible, then refresh its graph.
  async function refresh() {
    loading.value = true;
    error.value = null;

    try {
      const list = await fetchPipelines();
      pipelines.value = list;

      if (selectedPipelineId.value && !list.some((p) => p.id === selectedPipelineId.value)) {
        selectedPipelineId.value = null;
      }

      if (!selectedPipelineId.value && list.length > 0) {
        selectedPipelineId.value = list[0].id;
      }

      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    } finally {
      loading.value = false;
    }
  }

  async function selectPipeline(id: string | null) {
    selectedPipelineId.value = id;
    selectedEdgeId.value = null;
    selectedNodeId.value = null;
    loading.value = true;
    error.value = null;

    try {
      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    } finally {
      loading.value = false;
    }
  }

  async function createPipeline(name: string, description: string): Promise<Pipeline | null> {
    const trimmed = name.trim();

    if (!trimmed) {
      return null;
    }

    try {
      const saved = await savePipeline({
        id: null,
        name: trimmed,
        description: description.trim() || null,
        workflow_ids: [],
        member_failure_modes: {},
        defaults: defaultPipelineDefaults(),
        metadata: {},
      });
      pipelines.value = [...pipelines.value, saved];
      await selectPipeline(saved.id);
      return saved;
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
      return null;
    }
  }

  // persist a mutation to the selected pipeline record, then re-derive the graph.
  async function persistSelected(mutate: (draft: Pipeline) => Pipeline): Promise<void> {
    const current = selectedPipeline.value;

    if (!current) {
      return;
    }

    try {
      const saved = await savePipeline(mutate({ ...current }));
      pipelines.value = pipelines.value.map((p) => (p.id === saved.id ? saved : p));
      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  function renamePipeline(name: string, description: string | null) {
    return persistSelected((draft) => ({ ...draft, name: name.trim(), description }));
  }

  function savePipelineDefaults(defaults: PipelineDefaults) {
    return persistSelected((draft) => ({ ...draft, defaults }));
  }

  function addWorkflowToPipeline(workflowId: string) {
    return persistSelected((draft) => {
      if (draft.workflow_ids.includes(workflowId)) {
        return draft;
      }

      return { ...draft, workflow_ids: [...draft.workflow_ids, workflowId] };
    });
  }

  function withoutMemberFailureMode(
    modes: Record<string, PipelineMemberFailureMode>,
    workflowId: string,
  ): Record<string, PipelineMemberFailureMode> {
    return Object.fromEntries(Object.entries(modes).filter(([id]) => id !== workflowId));
  }

  function removeWorkflowFromPipeline(workflowId: string) {
    return persistSelected((draft) => ({
      ...draft,
      workflow_ids: draft.workflow_ids.filter((id) => id !== workflowId),
      member_failure_modes: withoutMemberFailureMode(draft.member_failure_modes, workflowId),
    }));
  }

  // set (or clear, when `mode` is null) a member's failure-mode override.
  function setMemberFailureMode(workflowId: string, mode: PipelineMemberFailureMode | null) {
    return persistSelected((draft) => {
      const memberFailureModes =
        mode === null
          ? withoutMemberFailureMode(draft.member_failure_modes, workflowId)
          : { ...draft.member_failure_modes, [workflowId]: mode };

      return { ...draft, member_failure_modes: memberFailureModes };
    });
  }

  // reassign the selected pipeline's owning org (null = platform-global).
  async function setPipelineOwner(orgId: string | null) {
    const current = selectedPipeline.value;

    if (!current?.id) {
      return;
    }

    try {
      const saved = await setPipelineOwnerService(current.id, orgId);
      pipelines.value = pipelines.value.map((p) => (p.id === saved.id ? saved : p));
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  async function deletePipeline(id: string) {
    try {
      await deletePipelineService(id);
      pipelines.value = pipelines.value.filter((p) => p.id !== id);

      if (selectedPipelineId.value === id) {
        selectedPipelineId.value = pipelines.value[0]?.id ?? null;
        await refreshGraph();
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  const selectedEdge = computed(
    () => edges.value.find((edge) => edge.id === selectedEdgeId.value) ?? null,
  );

  const selectedNode = computed(
    () => nodes.value.find((node) => node.id === selectedNodeId.value) ?? null,
  );

  function nameById(id: string): string {
    return allWorkflows.value.find((wf) => wf.id === id)?.name ?? id;
  }

  function triggerForEdge(edge: PipelineEdgeModel): WorkflowTrigger | null {
    const list = triggersByWorkflowId.value[edge.data.sourceWorkflowId] ?? [];
    return list.find((trigger) => trigger.id === edge.data.triggerId) ?? null;
  }

  async function createLink(sourceId: string, targetId: string) {
    const pipeline = selectedPipeline.value;

    if (!pipeline) {
      return;
    }

    // skip an exact duplicate of the default link; other selectors are edited after.
    const on = pipeline.defaults.on_step_failure === "continue" ? "complete" : "success";
    const duplicate = edges.value.some(
      (edge) => edge.source === sourceId && edge.target === targetId && edge.data.on === on,
    );

    if (duplicate) {
      return;
    }

    try {
      await createChainLink(sourceId, nameById(targetId), pipeline);
      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  async function updateSelected(changes: { on?: ChainEvent; enabled?: boolean }) {
    const edge = selectedEdge.value;
    const trigger = edge ? triggerForEdge(edge) : null;

    if (!trigger) {
      return;
    }

    try {
      await updateChainLink(trigger, changes);
      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  async function deleteSelected() {
    const edge = selectedEdge.value;

    if (!edge?.data.triggerId) {
      return;
    }

    try {
      await deleteChainLink(edge.data.triggerId);
      selectedEdgeId.value = null;
      await refreshGraph();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  return {
    pipelines,
    selectedPipelineId,
    selectedPipeline,
    memberWorkflows,
    availableWorkflows,
    nodes,
    edges,
    unresolved,
    selectedEdgeId,
    selectedEdge,
    selectedNodeId,
    selectedNode,
    loading,
    error,
    refresh,
    selectPipeline,
    createPipeline,
    renamePipeline,
    savePipelineDefaults,
    addWorkflowToPipeline,
    removeWorkflowFromPipeline,
    setMemberFailureMode,
    setPipelineOwner,
    deletePipeline,
    createLink,
    updateSelected,
    deleteSelected,
    nameById,
    selectEdge: (id: string | null) => {
      selectedEdgeId.value = id;
      selectedNodeId.value = null;
    },
    selectNode: (id: string | null) => {
      selectedNodeId.value = id;
      selectedEdgeId.value = null;
    },
  };
});
