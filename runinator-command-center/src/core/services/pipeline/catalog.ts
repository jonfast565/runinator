import {
  deletePipeline as deletePipelineApi,
  fetchPipelines as fetchPipelinesApi,
  savePipeline as savePipelineApi,
  setPipelineOwner as setPipelineOwnerApi,
} from "../../api/commandCenterApi";
import type { Pipeline } from "../../domain/models";

/** Pipeline definitions and ownership. */
export async function fetchPipelines(): Promise<Pipeline[]> {
  return fetchPipelinesApi();
}

export async function savePipeline(pipeline: Pipeline): Promise<Pipeline> {
  return savePipelineApi(pipeline);
}

export async function deletePipeline(pipelineId: string): Promise<void> {
  await deletePipelineApi(pipelineId);
}

/** Reassign a pipeline's owning organization; null makes it platform-global. */
export async function setPipelineOwner(
  pipelineId: string,
  orgId: string | null,
): Promise<Pipeline> {
  return setPipelineOwnerApi(pipelineId, orgId);
}
