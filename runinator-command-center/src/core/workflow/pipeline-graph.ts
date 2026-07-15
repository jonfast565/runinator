// portable model builders for the pipeline graph: workflows as nodes, chained triggers as edges.
// no vue/pinia imports so this stays unit-testable.

import type { JsonRecord } from "../domain/json";
import type { WorkflowDefinition } from "../domain/models";
import type { WorkflowTrigger } from "../domain/models/workflow/trigger";
import { autoArrangeWorkflowLayout } from "./index";

export type ChainEvent = "success" | "failure" | "complete";

export interface PipelineNodeData {
  workflowId: string;
  name: string;
  enabled: boolean;
  outgoing: number;
  incoming: number;
}

export interface PipelineNodeModel {
  id: string;
  type: "pipeline";
  position: { x: number; y: number };
  data: PipelineNodeData;
}

export interface PipelineEdgeData {
  triggerId: string | null;
  sourceWorkflowId: string;
  targetName: string;
  on: ChainEvent;
  enabled: boolean;
}

export interface PipelineEdgeModel {
  id: string;
  type: "pipeline";
  source: string;
  target: string;
  label: string;
  data: PipelineEdgeData;
  animated: boolean;
  class?: string;
}

/** a chained trigger whose `target_workflow` name does not match any known workflow. */
export interface UnresolvedChain {
  triggerId: string | null;
  sourceWorkflowId: string;
  sourceName: string;
  targetName: string;
  on: ChainEvent;
  enabled: boolean;
}

export interface PipelineGraph {
  nodes: PipelineNodeModel[];
  edges: PipelineEdgeModel[];
  unresolved: UnresolvedChain[];
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function chainEvent(value: unknown): ChainEvent {
  return value === "failure" || value === "complete" ? value : "success";
}

export interface BuildPipelineGraphOptions {
  /** when set, only member workflows are nodes and only links tagged with this pipeline are edges. */
  pipelineId?: string | null;
  /** the member workflow ids that scope the graph (used with `pipelineId`). */
  memberIds?: string[];
}

/** does a chained trigger belong to the pipeline currently being rendered? */
function belongsToPipeline(trigger: WorkflowTrigger, pipelineId: string | null | undefined): boolean {
  if (pipelineId == null) {
    return true;
  }

  return asString(trigger.configuration.pipeline_id) === pipelineId;
}

/** build the workflow-level graph: one node per workflow, one edge per resolvable chained trigger. */
export function buildPipelineGraph(
  workflows: WorkflowDefinition[],
  triggersByWorkflowId: Record<string, WorkflowTrigger[]>,
  options: BuildPipelineGraphOptions = {},
): PipelineGraph {
  const memberSet =
    options.memberIds != null ? new Set(options.memberIds) : null;
  const identified = workflows.filter(
    (wf): wf is WorkflowDefinition & { id: string } =>
      wf.id != null && (memberSet == null || memberSet.has(wf.id)),
  );
  const nameToId = new Map<string, string>();

  for (const wf of identified) {
    nameToId.set(wf.name, wf.id);
  }

  const nameById = new Map(identified.map((wf) => [wf.id, wf.name]));

  const edges: PipelineEdgeModel[] = [];
  const unresolved: UnresolvedChain[] = [];
  const outgoing = new Map<string, number>();
  const incoming = new Map<string, number>();
  // synthetic transitions for the auto-layout: each workflow's resolved chain targets as branches.
  const branchesByWorkflow = new Map<string, { target: { $node: string } }[]>();

  for (const wf of identified) {
    for (const trigger of triggersByWorkflowId[wf.id] ?? []) {
      if (trigger.kind !== "chained") {
        continue;
      }

      if (!belongsToPipeline(trigger, options.pipelineId)) {
        continue;
      }

      const on = chainEvent(trigger.configuration.on);
      const targetName = asString(trigger.configuration.target_workflow);
      const targetId = nameToId.get(targetName);

      if (!targetId) {
        unresolved.push({
          triggerId: trigger.id,
          sourceWorkflowId: wf.id,
          sourceName: wf.name,
          targetName,
          on,
          enabled: trigger.enabled,
        });
        continue;
      }

      edges.push({
        id: trigger.id ?? `${wf.id}->${targetId}:${on}`,
        type: "pipeline",
        source: wf.id,
        target: targetId,
        label: `on ${on}`,
        animated: trigger.enabled,
        class: trigger.enabled ? undefined : "pipeline-edge-disabled",
        data: {
          triggerId: trigger.id,
          sourceWorkflowId: wf.id,
          targetName,
          on,
          enabled: trigger.enabled,
        },
      });
      outgoing.set(wf.id, (outgoing.get(wf.id) ?? 0) + 1);
      incoming.set(targetId, (incoming.get(targetId) ?? 0) + 1);
      const branches = branchesByWorkflow.get(wf.id) ?? [];
      branches.push({ target: { $node: targetId } });
      branchesByWorkflow.set(wf.id, branches);
    }
  }

  const layoutDefinition: JsonRecord = {
    nodes: identified.map((wf) => ({
      id: wf.id,
      transitions: { branches: branchesByWorkflow.get(wf.id) ?? [] },
    })),
  };
  const positions = autoArrangeWorkflowLayout(layoutDefinition, "horizontal");

  const nodes: PipelineNodeModel[] = identified.map((wf) => ({
    id: wf.id,
    type: "pipeline",
    position: positions[wf.id] ?? { x: 0, y: 0 },
    data: {
      workflowId: wf.id,
      name: nameById.get(wf.id) ?? wf.name,
      enabled: wf.enabled,
      outgoing: outgoing.get(wf.id) ?? 0,
      incoming: incoming.get(wf.id) ?? 0,
    },
  }));

  return { nodes, edges, unresolved };
}
