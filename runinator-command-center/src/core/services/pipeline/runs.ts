import {
  cancelPipelineRun as cancelPipelineRunApi,
  createPipelineRun as createPipelineRunApi,
  deletePipelineRun as deletePipelineRunApi,
  fetchPipelineRun as fetchPipelineRunApi,
  fetchPipelineRuns as fetchPipelineRunsApi,
  pausePipelineRun as pausePipelineRunApi,
  resolvePipelineRun as resolvePipelineRunApi,
  resumePipelineRun as resumePipelineRunApi,
  retryPipelineMember as retryPipelineMemberApi,
} from "../../api/commandCenterApi";
import type { JsonRecord } from "../../domain/json";
import type { PipelineRun, PipelineRunDetail } from "../../domain/models";
import type { ManagedRunOverrideOptions } from "../../api/commandCenterApi";

/** Start a manual run of a pipeline (starts its entry members). */
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

export async function deletePipelineRun(pipelineRunId: string): Promise<void> {
  const response = await deletePipelineRunApi(pipelineRunId);

  if (!response.success) {
    throw new Error(response.message || "Failed to delete pipeline run");
  }
}

export async function cancelPipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
): Promise<void> {
  await cancelPipelineRunApi(pipelineRunId, override);
}

export async function pausePipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
): Promise<void> {
  await pausePipelineRunApi(pipelineRunId, override);
}

export async function resumePipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
): Promise<void> {
  await resumePipelineRunApi(pipelineRunId, override);
}

export async function retryPipelineMember(
  pipelineRunId: string,
  memberKey: string,
  parameters: JsonRecord = {},
  override?: ManagedRunOverrideOptions,
) {
  return retryPipelineMemberApi(pipelineRunId, memberKey, parameters, override);
}

/** Resolve a pipeline run's pending inquiry. */
export async function resolvePipelineRun(
  pipelineRunId: string,
  decision: "continue" | "abort",
  message?: string | null,
): Promise<PipelineRun> {
  return resolvePipelineRunApi(pipelineRunId, decision, null, message ?? null);
}
