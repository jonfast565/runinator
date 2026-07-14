import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { WorkflowDefinition, WorkflowTrigger } from "../../../../core/domain/models";
import {
  createChainLink,
  deleteChainLink,
  loadPipelineData,
  updateChainLink,
} from "../../../../core/services/pipeline";
import {
  buildPipelineGraph,
  type ChainEvent,
  type PipelineEdgeModel,
  type PipelineNodeModel,
  type UnresolvedChain,
} from "../../../../core/workflow/pipeline-graph";

// the pipeline graph: workflows as nodes, chained triggers as edges. state is local to this store
// (it doesn't mirror a long-lived service); reads via loadPipelineData, writes via the chain-link
// helpers, then re-derives the graph from the refreshed data.
export const usePipelineStore = defineStore("pipeline", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const triggersByWorkflowId = ref<Record<string, WorkflowTrigger[]>>({});
  const nodes = ref<PipelineNodeModel[]>([]);
  const edges = ref<PipelineEdgeModel[]>([]);
  const unresolved = ref<UnresolvedChain[]>([]);
  const selectedEdgeId = ref<string | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  function rebuild() {
    const graph = buildPipelineGraph(workflows.value, triggersByWorkflowId.value);
    nodes.value = graph.nodes;
    edges.value = graph.edges;
    unresolved.value = graph.unresolved;

    if (selectedEdgeId.value && !edges.value.some((edge) => edge.id === selectedEdgeId.value)) {
      selectedEdgeId.value = null;
    }
  }

  async function refresh() {
    loading.value = true;
    error.value = null;

    try {
      const data = await loadPipelineData();
      workflows.value = data.workflows;
      triggersByWorkflowId.value = data.triggersByWorkflowId;
      rebuild();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    } finally {
      loading.value = false;
    }
  }

  const selectedEdge = computed(
    () => edges.value.find((edge) => edge.id === selectedEdgeId.value) ?? null,
  );

  function nameById(id: string): string {
    return workflows.value.find((wf) => wf.id === id)?.name ?? id;
  }

  function triggerForEdge(edge: PipelineEdgeModel): WorkflowTrigger | null {
    const list = triggersByWorkflowId.value[edge.data.sourceWorkflowId] ?? [];
    return list.find((trigger) => trigger.id === edge.data.triggerId) ?? null;
  }

  async function createLink(sourceId: string, targetId: string) {
    // skip an exact duplicate of the default (on:success) link; other selectors are edited after.
    const duplicate = edges.value.some(
      (edge) => edge.source === sourceId && edge.target === targetId && edge.data.on === "success",
    );

    if (duplicate) {
      return;
    }

    try {
      await createChainLink(sourceId, nameById(targetId), "success");
      await refresh();
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
      await refresh();
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
      await refresh();
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  return {
    nodes,
    edges,
    unresolved,
    selectedEdgeId,
    selectedEdge,
    loading,
    error,
    refresh,
    createLink,
    updateSelected,
    deleteSelected,
    nameById,
    selectEdge: (id: string | null) => {
      selectedEdgeId.value = id;
    },
  };
});
