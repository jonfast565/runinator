import { defaultApi, type PipelineCatalogApi } from "../../api/ports/pipeline-catalog";

import type { Pipeline } from "../../domain/models";

/** Pipeline definitions and ownership. */
export function createPipelineCatalogService(api: PipelineCatalogApi = defaultApi) {
  async function fetchPipelines(): Promise<Pipeline[]> {
    return api.fetchPipelines();
  }

  async function savePipeline(pipeline: Pipeline): Promise<Pipeline> {
    return api.savePipeline(pipeline);
  }

  async function fetchPipelineRexRap(pipelineId: string): Promise<string> {
    return api.fetchPipelineRexRap(pipelineId);
  }

  async function savePipelineRexRap(pipelineId: string, source: string): Promise<Pipeline> {
    return api.savePipelineRexRap(pipelineId, source);
  }

  async function deletePipeline(pipelineId: string): Promise<void> {
    await api.deletePipeline(pipelineId);
  }

  /** Reassign a pipeline's owning organization; null makes it platform-global. */
  async function setPipelineOwner(pipelineId: string, orgId: string | null): Promise<Pipeline> {
    return api.setPipelineOwner(pipelineId, orgId);
  }

  return {
    fetchPipelines,
    savePipeline,
    fetchPipelineRexRap,
    savePipelineRexRap,
    deletePipeline,
    setPipelineOwner,
  };
}

export const {
  fetchPipelines,
  savePipeline,
  fetchPipelineRexRap,
  savePipelineRexRap,
  deletePipeline,
  setPipelineOwner,
} = createPipelineCatalogService();
