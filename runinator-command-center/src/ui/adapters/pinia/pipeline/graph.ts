import type { Pipeline, WorkflowDefinition, WorkflowTrigger } from "../../../../core/domain/models";
import {
  buildPipelineGraph,
  type PipelineEdgeModel,
  type PipelineNodeModel,
  type UnresolvedChain,
} from "../../../../core/workflow/pipeline-graph";

export interface PipelinePresentation {
  nodes: PipelineNodeModel[];
  edges: PipelineEdgeModel[];
  unresolved: UnresolvedChain[];
}

/** Derive Vue Flow presentation state from the persisted pipeline graph. */
export function buildPipelinePresentation(
  pipeline: Pipeline | null,
  workflows: WorkflowDefinition[],
): PipelinePresentation {
  if (!pipeline) {
    return { nodes: [], edges: [], unresolved: [] };
  }

  const syntheticTriggers: Record<string, WorkflowTrigger[]> = {};

  const memberByKey = new Map(pipeline.graph.members.map((member) => [member.key, member]));

  for (const link of pipeline.graph.links) {
    const source = memberByKey.get(link.from);

    if (!source) {
      continue;
    }

    (syntheticTriggers[source.workflow_id] ??= []).push({
      id: link.id,
      workflow_id: source.workflow_id,
      kind: "chained",
      enabled: link.enabled,
      configuration: {
        on: link.on,
        target_workflow: link.to,
        parameters: link.parameters,
        pipeline_id: pipeline.id,
      },
      next_execution: null,
      blackout_start: null,
      blackout_end: null,
      metadata: {},
    });
  }

  return buildPipelineGraph(workflows, syntheticTriggers, {
    pipelineId: pipeline.id,
    memberIds: pipeline.graph.members.map((member) => member.workflow_id),
    memberFailureModes: Object.fromEntries(
      pipeline.graph.members.map((member) => [member.workflow_id, member.failure_mode]),
    ),
    defaultFailureMode: pipeline.defaults.default_failure_mode,
  });
}
