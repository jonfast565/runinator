import { defaultApi, type PipelineTriggersApi } from "../../api/ports/pipeline-triggers";

import type { PipelineTrigger } from "../../domain/models";

/** Triggers attached directly to a pipeline. */
export function createPipelineTriggersService(api: PipelineTriggersApi = defaultApi) {
  async function fetchPipelineTriggers(pipelineId: string): Promise<PipelineTrigger[]> {
    return api.fetchPipelineTriggers(pipelineId);
  }

  async function savePipelineTrigger(
    trigger: PipelineTrigger,
    creating: boolean,
  ): Promise<PipelineTrigger> {
    return api.savePipelineTrigger(trigger, creating);
  }

  async function deletePipelineTrigger(triggerId: string): Promise<void> {
    await api.deletePipelineTrigger(triggerId);
  }

  return { fetchPipelineTriggers, savePipelineTrigger, deletePipelineTrigger };
}

export const { fetchPipelineTriggers, savePipelineTrigger, deletePipelineTrigger } =
  createPipelineTriggersService();
