import {
  deletePipelineTrigger as deletePipelineTriggerApi,
  fetchPipelineTriggers as fetchPipelineTriggersApi,
  savePipelineTrigger as savePipelineTriggerApi,
} from "../../api/commandCenterApi";
import type { PipelineTrigger } from "../../domain/models";

/** Triggers attached directly to a pipeline. */
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
