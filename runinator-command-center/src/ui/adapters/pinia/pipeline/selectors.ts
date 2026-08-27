import { computed, type Ref } from "vue";
import type { Pipeline, WorkflowDefinition } from "../../../../core/domain/models";
import type {
  PipelineEdgeModel,
  PipelineNodeModel,
  UnresolvedChain,
} from "../../../../core/workflow/pipeline-graph";

interface PipelineSelectorDependencies {
  pipelines: Ref<Pipeline[]>;
  selectedPipelineId: Ref<string | null>;
  allWorkflows: Ref<WorkflowDefinition[]>;
  edges: Ref<PipelineEdgeModel[]>;
  nodes: Ref<PipelineNodeModel[]>;
  selectedEdgeId: Ref<string | null>;
  selectedNodeId: Ref<string | null>;
  unresolved: Ref<UnresolvedChain[]>;
}

/** Read-only projections for pipeline editor components. */
export function createPipelineSelectors(deps: PipelineSelectorDependencies) {
  const selectedPipeline = computed(
    () =>
      deps.pipelines.value.find((pipeline) => pipeline.id === deps.selectedPipelineId.value) ??
      null,
  );
  const memberWorkflows = computed(() => {
    const ids = new Set(
      selectedPipeline.value?.graph.members.map((member) => member.workflow_id) ?? [],
    );
    return deps.allWorkflows.value.filter(
      (workflow): workflow is WorkflowDefinition & { id: string } =>
        workflow.id != null && ids.has(workflow.id),
    );
  });
  const availableWorkflows = computed(() => {
    const ids = new Set(
      selectedPipeline.value?.graph.members.map((member) => member.workflow_id) ?? [],
    );
    return deps.allWorkflows.value.filter(
      (workflow): workflow is WorkflowDefinition & { id: string } =>
        workflow.id != null && !ids.has(workflow.id),
    );
  });
  const selectedEdge = computed(
    () => deps.edges.value.find((edge) => edge.id === deps.selectedEdgeId.value) ?? null,
  );
  const selectedNode = computed(
    () => deps.nodes.value.find((node) => node.id === deps.selectedNodeId.value) ?? null,
  );

  return {
    selectedPipeline,
    memberWorkflows,
    availableWorkflows,
    selectedEdge,
    selectedNode,
    unresolved: deps.unresolved,
  };
}
