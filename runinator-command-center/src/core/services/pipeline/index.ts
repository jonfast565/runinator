// data + write helpers behind the pipeline canvas. a pipeline is a named instance grouping member
// workflows; the links between members are `chained` workflow triggers stamped with the pipeline's
// id. there is no bulk "all triggers" endpoint, so member triggers are fetched per workflow and
// fanned out (same pattern as the pack export).

import {
  cancelPipelineRun as cancelPipelineRunApi,
  createPipelineRun as createPipelineRunApi,
  deletePipeline as deletePipelineApi,
  deletePipelineTrigger as deletePipelineTriggerApi,
  deleteWorkflowTrigger,
  fetchPipelineRun as fetchPipelineRunApi,
  fetchPipelineRuns as fetchPipelineRunsApi,
  fetchPipelineTriggers as fetchPipelineTriggersApi,
  fetchPipelines as fetchPipelinesApi,
  fetchWorkflowTriggers,
  fetchWorkflows,
  savePipeline as savePipelineApi,
  savePipelineTrigger as savePipelineTriggerApi,
  setPipelineOwner as setPipelineOwnerApi,
  saveWorkflowTrigger,
} from "../../api/commandCenterApi";
import type { JsonRecord } from "../../domain/json";
import type {
  Pipeline,
  PipelineRun,
  PipelineRunDetail,
  PipelineTrigger,
  WorkflowDefinition,
  WorkflowTrigger,
} from "../../domain/models";
import type { ChainEvent } from "../../workflow/pipeline-graph";

export interface PipelineData {
  workflows: WorkflowDefinition[];
  triggersByWorkflowId: Record<string, WorkflowTrigger[]>;
}

/** the `on` selector a new link inherits from the pipeline's failure policy. */
export function defaultChainEvent(pipeline: Pipeline): ChainEvent {
  return pipeline.defaults.on_step_failure === "continue" ? "complete" : "success";
}

/** load the full workflow list (for the picker + name resolution) and triggers for the members. */
export async function loadPipelineData(memberIds: string[]): Promise<PipelineData> {
  const workflows = await fetchWorkflows();
  const memberSet = new Set(memberIds);
  const members = workflows.filter(
    (wf): wf is WorkflowDefinition & { id: string } => wf.id != null && memberSet.has(wf.id),
  );
  const triggerLists = await Promise.all(members.map((wf) => fetchWorkflowTriggers(wf.id)));
  const triggersByWorkflowId: Record<string, WorkflowTrigger[]> = {};
  members.forEach((wf, index) => {
    triggersByWorkflowId[wf.id] = triggerLists[index];
  });
  return { workflows, triggersByWorkflowId };
}

export async function fetchPipelines(): Promise<Pipeline[]> {
  return fetchPipelinesApi();
}

export async function savePipeline(pipeline: Pipeline): Promise<Pipeline> {
  return savePipelineApi(pipeline);
}

export async function deletePipeline(pipelineId: string): Promise<void> {
  await deletePipelineApi(pipelineId);
}

/** reassign a pipeline's owning organization; null makes it platform-global. */
export async function setPipelineOwner(
  pipelineId: string,
  orgId: string | null,
): Promise<Pipeline> {
  return setPipelineOwnerApi(pipelineId, orgId);
}

// --- pipeline triggers (cron/manual/chained on the pipeline itself) ---

export async function fetchPipelineTriggers(pipelineId: string): Promise<PipelineTrigger[]> {
  return fetchPipelineTriggersApi(pipelineId);
}

export async function savePipelineTrigger(
  trigger: PipelineTrigger,
  creating: boolean,
): Promise<PipelineTrigger> {
  return savePipelineTriggerApi(trigger, creating);
}

export async function deletePipelineTrigger(triggerId: string): Promise<void> {
  await deletePipelineTriggerApi(triggerId);
}

// --- pipeline runs ---

/** start a manual run of a pipeline (starts its entry members). */
export async function createPipelineRun(
  pipelineId: string,
  parameters: JsonRecord = {},
): Promise<PipelineRun> {
  return createPipelineRunApi(pipelineId, parameters);
}

export async function fetchPipelineRuns(): Promise<PipelineRun[]> {
  return fetchPipelineRunsApi();
}

export async function fetchPipelineRun(pipelineRunId: string): Promise<PipelineRunDetail> {
  return fetchPipelineRunApi(pipelineRunId);
}

export async function cancelPipelineRun(pipelineRunId: string): Promise<void> {
  await cancelPipelineRunApi(pipelineRunId);
}

/** create a chained trigger tagged with the pipeline, pre-filled from the pipeline's defaults. */
export async function createChainLink(
  sourceWorkflowId: string,
  targetName: string,
  pipeline: Pipeline,
): Promise<WorkflowTrigger> {
  const configuration: JsonRecord = {
    on: defaultChainEvent(pipeline),
    target_workflow: targetName,
    parameters: pipeline.defaults.default_parameters,
    pipeline_id: pipeline.id,
  };

  if (pipeline.defaults.max_chain_depth != null) {
    configuration.max_chain_depth = pipeline.defaults.max_chain_depth;
  }

  const trigger: WorkflowTrigger = {
    id: null,
    workflow_id: sourceWorkflowId,
    kind: "chained",
    enabled: pipeline.defaults.links_enabled_by_default,
    configuration,
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {},
  };
  return saveWorkflowTrigger(trigger, true);
}

/** persist selector/enabled edits to an existing chained trigger (pipeline tag preserved). */
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
