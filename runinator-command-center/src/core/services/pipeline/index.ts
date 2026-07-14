// loads the data behind the pipeline graph: every workflow plus its triggers. there is no bulk
// "all triggers" endpoint, so triggers are fetched per workflow and fanned out (same pattern as
// the pack export).

import {
  deleteWorkflowTrigger,
  fetchWorkflowTriggers,
  fetchWorkflows,
  saveWorkflowTrigger,
} from "../../api/commandCenterApi";
import type { WorkflowDefinition, WorkflowTrigger } from "../../domain/models";
import type { ChainEvent } from "../../workflow/pipeline-graph";

export interface PipelineData {
  workflows: WorkflowDefinition[];
  triggersByWorkflowId: Record<string, WorkflowTrigger[]>;
}

export async function loadPipelineData(): Promise<PipelineData> {
  const workflows = await fetchWorkflows();
  const identified = workflows.filter(
    (wf): wf is WorkflowDefinition & { id: string } => Boolean(wf.id),
  );
  const triggerLists = await Promise.all(
    identified.map((wf) => fetchWorkflowTriggers(wf.id)),
  );
  const triggersByWorkflowId: Record<string, WorkflowTrigger[]> = {};
  identified.forEach((wf, index) => {
    triggersByWorkflowId[wf.id] = triggerLists[index];
  });
  return { workflows, triggersByWorkflowId };
}

/** create a chained trigger on `sourceWorkflowId` targeting `targetName` when it reaches `on`. */
export async function createChainLink(
  sourceWorkflowId: string,
  targetName: string,
  on: ChainEvent = "success",
): Promise<WorkflowTrigger> {
  const trigger: WorkflowTrigger = {
    id: null,
    workflow_id: sourceWorkflowId,
    kind: "chained",
    enabled: true,
    configuration: { on, target_workflow: targetName, parameters: {} },
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {},
  };
  return saveWorkflowTrigger(trigger, true);
}

/** persist selector/enabled edits to an existing chained trigger. */
export async function updateChainLink(
  trigger: WorkflowTrigger,
  changes: { on?: ChainEvent; enabled?: boolean },
): Promise<WorkflowTrigger> {
  const next: WorkflowTrigger = {
    ...trigger,
    enabled: changes.enabled ?? trigger.enabled,
    configuration: {
      ...trigger.configuration,
      on: changes.on ?? trigger.configuration.on,
    },
  };
  return saveWorkflowTrigger(next, false);
}

export async function deleteChainLink(triggerId: string): Promise<void> {
  await deleteWorkflowTrigger(triggerId);
}
