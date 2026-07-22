import { MarkerType, type Edge, type Node } from "@vue-flow/core";
import type {
  GraphEdgeModel,
  GraphNodeModel,
} from "../../../core/workflow/graph-model";
import {
  buildGraphEdgeModels,
  buildGraphNodeModels,
} from "../../../core/workflow/index";
import type {
  ProviderMetadata,
  WorkflowDefinition,
  WorkflowRunDetail,
} from "../../../core/domain/models";

export function toVueFlowNode(model: GraphNodeModel): Node {
  return model as Node;
}

export function toVueFlowEdge(model: GraphEdgeModel): Edge {
  return {
    ...model,
    markerEnd: MarkerType.ArrowClosed,
  } as Edge;
}

export function buildGraphNodes(
  workflow: WorkflowDefinition,
  detail: WorkflowRunDetail | null,
  subflowNames?: Map<string, string>,
  providers: ProviderMetadata[] = [],
): Node[] {
  return buildGraphNodeModels(workflow, detail, subflowNames, providers).map(toVueFlowNode);
}

export function buildGraphEdges(
  workflow: WorkflowDefinition,
  completedNodeIds?: ReadonlySet<string> | null,
  traversedKeys?: ReadonlySet<string> | null,
  activeNodeIds?: ReadonlySet<string> | null,
): Edge[] {
  return buildGraphEdgeModels(workflow, completedNodeIds, traversedKeys, activeNodeIds).map(
    toVueFlowEdge,
  );
}
