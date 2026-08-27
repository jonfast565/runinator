import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  Pipeline,
  PipelineDefaults,
  PipelineConcurrency,
  PipelineJoinMode,
  PipelineMemberFailureMode,
  WorkflowDefinition,
} from "../../../../core/domain/models";
import { defaultPipelineDefaults } from "../../../../core/domain/models";
import { workflowPath } from "../../../../core/domain/models";
import {
  deletePipeline as deletePipelineService,
  fetchPipelines,
  loadPipelineData,
  savePipeline,
  setPipelineOwner as setPipelineOwnerService,
} from "../../../../core/services/pipeline";
import type {
  ChainEvent,
  PipelineEdgeModel,
  PipelineNodeModel,
  UnresolvedChain,
} from "../../../../core/workflow/pipeline-graph";
import { buildPipelinePresentation } from "./graph";
import { createPipelineSelectors } from "./selectors";

// The pipeline canvas store. Members, links, joins, mappings, and concurrency are persisted on the
// pipeline graph; Vue Flow nodes and edges are derived presentation state.
export const usePipelineStore = defineStore("pipeline", () => {
  const pipelines = ref<Pipeline[]>([]);
  const selectedPipelineId = ref<string | null>(null);
  const allWorkflows = ref<WorkflowDefinition[]>([]);
  const nodes = ref<PipelineNodeModel[]>([]);
  const edges = ref<PipelineEdgeModel[]>([]);
  const unresolved = ref<UnresolvedChain[]>([]);
  const selectedEdgeId = ref<string | null>(null);
  const selectedNodeId = ref<string | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const selectors = createPipelineSelectors({
    pipelines,
    selectedPipelineId,
    allWorkflows,
    edges,
    nodes,
    selectedEdgeId,
    selectedNodeId,
    unresolved,
  });
  const { selectedPipeline, memberWorkflows, availableWorkflows, selectedEdge, selectedNode } =
    selectors;

  function rebuild() {
    const pipeline = selectedPipeline.value;

    const graph = buildPipelinePresentation(pipeline, allWorkflows.value);
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

    const data = await loadPipelineData(pipeline.graph.members.map((member) => member.workflow_id));
    allWorkflows.value = data.workflows;
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

  async function createPipeline(
    name: string,
    namespace: string,
    key: string,
    description: string,
  ): Promise<Pipeline | null> {
    const trimmed = name.trim();
    const trimmedNamespace = namespace.trim();
    const trimmedKey = key.trim();

    if (!trimmed || !trimmedNamespace || !trimmedKey) {
      return null;
    }

    try {
      const saved = await savePipeline({
        id: null,
        name: trimmed,
        namespace: trimmedNamespace,
        key: trimmedKey,
        description: description.trim() || null,
        graph: { version: 1, members: [], links: [], joins: {} },
        concurrency: { max_concurrent_runs: 0, on_conflict: "allow" },
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

  function renamePipeline(
    name: string,
    description: string | null,
    namespace: string,
    key: string,
  ) {
    return persistSelected((draft) => ({
      ...draft,
      name: name.trim(),
      description,
      namespace: namespace.trim(),
      key: key.trim(),
    }));
  }

  function savePipelineDefaults(defaults: PipelineDefaults) {
    return persistSelected((draft) => ({ ...draft, defaults }));
  }

  function savePipelineConcurrency(concurrency: PipelineConcurrency) {
    return persistSelected((draft) => ({ ...draft, concurrency }));
  }

  function updateJoin(target: string, mode: PipelineJoinMode, parameters: Record<string, unknown>) {
    return persistSelected((draft) => ({
      ...draft,
      graph: {
        ...draft.graph,
        joins: {
          ...draft.graph.joins,
          [target]: { target, mode, parameters },
        },
      },
    }));
  }

  function addWorkflowToPipeline(workflowId: string) {
    return persistSelected((draft) => {
      if (draft.graph.members.some((member) => member.workflow_id === workflowId)) {
        return draft;
      }

      const workflow = allWorkflows.value.find((item) => item.id === workflowId);

      if (!workflow) {
        return draft;
      }

      return {
        ...draft,
        graph: {
          ...draft.graph,
          members: [
            ...draft.graph.members,
            {
              key: workflowPath(workflow),
              workflow_id: workflowId,
              failure_mode: draft.defaults.default_failure_mode,
            },
          ],
        },
      };
    });
  }

  function removeWorkflowFromPipeline(workflowId: string) {
    return persistSelected((draft) => ({
      ...draft,
      graph: {
        ...draft.graph,
        members: draft.graph.members.filter((member) => member.workflow_id !== workflowId),
        links: draft.graph.links.filter((link) => {
          const removed = draft.graph.members.find(
            (member) => member.workflow_id === workflowId,
          )?.key;
          return link.from !== removed && link.to !== removed;
        }),
        joins: Object.fromEntries(
          Object.entries(draft.graph.joins).filter(
            ([target]) =>
              draft.graph.members.find((member) => member.workflow_id === workflowId)?.key !==
              target,
          ),
        ),
      },
    }));
  }

  // set (or clear, when `mode` is null) a member's failure-mode override.
  function setMemberFailureMode(workflowId: string, mode: PipelineMemberFailureMode | null) {
    return persistSelected((draft) => {
      return {
        ...draft,
        graph: {
          ...draft.graph,
          members: draft.graph.members.map((member) =>
            member.workflow_id === workflowId
              ? { ...member, failure_mode: mode ?? draft.defaults.default_failure_mode }
              : member,
          ),
        },
      };
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

  function nameById(id: string): string {
    return allWorkflows.value.find((wf) => wf.id === id)?.name ?? id;
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
      const source = pipeline.graph.members.find((member) => member.workflow_id === sourceId);
      const target = pipeline.graph.members.find((member) => member.workflow_id === targetId);

      if (!source || !target) {
        return;
      }

      await persistSelected((draft) => ({
        ...draft,
        graph: {
          ...draft.graph,
          links: [
            ...draft.graph.links,
            {
              id: crypto.randomUUID(),
              from: source.key,
              to: target.key,
              on,
              enabled: draft.defaults.links_enabled_by_default,
              parameters: draft.defaults.default_parameters,
            },
          ],
          joins: {
            ...draft.graph.joins,
            ...(draft.graph.links.filter((link) => link.enabled && link.to === target.key).length >=
            1
              ? {
                  [target.key]: draft.graph.joins[target.key] ?? {
                    target: target.key,
                    mode: "all",
                    parameters: {},
                  },
                }
              : {}),
          },
        },
      }));
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    }
  }

  async function updateSelected(changes: {
    on?: ChainEvent;
    enabled?: boolean;
    parameters?: Record<string, unknown>;
  }) {
    const edge = selectedEdge.value;

    if (!edge) {
      return;
    }

    try {
      await persistSelected((draft) => {
        const links = draft.graph.links.map((link) =>
          link.id === edge.id
            ? {
                ...link,
                on: changes.on ?? link.on,
                enabled: changes.enabled ?? link.enabled,
                parameters: changes.parameters ?? link.parameters,
              }
            : link,
        );
        const target = links.find((link) => link.id === edge.id)?.to;
        const enabledInbound = target
          ? links.filter((link) => link.enabled && link.to === target).length
          : 0;
        let joins = { ...draft.graph.joins };

        if (target && enabledInbound >= 2) {
          joins[target] ??= { target, mode: "all", parameters: {} };
        }

        if (target && enabledInbound < 2) {
          joins = Object.fromEntries(Object.entries(joins).filter(([key]) => key !== target));
        }

        return { ...draft, graph: { ...draft.graph, links, joins } };
      });
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
      await persistSelected((draft) => {
        const removed = draft.graph.links.find((link) => link.id === edge.id);
        const links = draft.graph.links.filter((link) => link.id !== edge.id);
        let joins = { ...draft.graph.joins };

        if (removed && links.filter((link) => link.enabled && link.to === removed.to).length < 2) {
          joins = Object.fromEntries(Object.entries(joins).filter(([key]) => key !== removed.to));
        }

        return { ...draft, graph: { ...draft.graph, links, joins } };
      });
      selectedEdgeId.value = null;
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
    savePipelineConcurrency,
    updateJoin,
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
