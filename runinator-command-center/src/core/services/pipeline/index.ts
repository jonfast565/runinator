// Data + write helpers behind the first-class pipeline graph and run monitor.

import {
  cancelPipelineRun as cancelPipelineRunApi,
  pausePipelineRun as pausePipelineRunApi,
  resumePipelineRun as resumePipelineRunApi,
  createPipelineRun as createPipelineRunApi,
  deletePipeline as deletePipelineApi,
  deletePipelineRun as deletePipelineRunApi,
  deletePipelineTrigger as deletePipelineTriggerApi,
  deleteWorkflowTrigger,
  fetchPipelineRun as fetchPipelineRunApi,
  fetchPipelineRuns as fetchPipelineRunsApi,
  fetchPipelineTriggers as fetchPipelineTriggersApi,
  fetchPipelines as fetchPipelinesApi,
  fetchWorkflowTriggers,
  fetchWorkflows,
  resolvePipelineRun as resolvePipelineRunApi,
  retryPipelineMember as retryPipelineMemberApi,
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

export async function deletePipelineRun(pipelineRunId: string): Promise<void> {
  const response = await deletePipelineRunApi(pipelineRunId);

  if (!response.success) {
    throw new Error(response.message || "Failed to delete pipeline run");
  }
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

export async function pausePipelineRun(pipelineRunId: string): Promise<void> {
  await pausePipelineRunApi(pipelineRunId);
}

export async function resumePipelineRun(pipelineRunId: string): Promise<void> {
  await resumePipelineRunApi(pipelineRunId);
}

export async function retryPipelineMember(
  pipelineRunId: string,
  memberKey: string,
  parameters: JsonRecord = {},
) {
  return retryPipelineMemberApi(pipelineRunId, memberKey, parameters);
}

/** resolve a pipeline run's pending inquiry (a member with the `inquire` failure mode paused it). */
export async function resolvePipelineRun(
  pipelineRunId: string,
  decision: "continue" | "abort",
  message?: string | null,
): Promise<PipelineRun> {
  return resolvePipelineRunApi(pipelineRunId, decision, null, message ?? null);
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
