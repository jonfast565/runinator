import { defaultApi, type PipelineDataApi } from "../../api/ports/pipeline-data";

import type { WorkflowDefinition, WorkflowTrigger } from "../../domain/models";

export interface PipelineData {
  workflows: WorkflowDefinition[];
  triggersByWorkflowId: Record<string, WorkflowTrigger[]>;
}

export function createPipelineDataService(api: PipelineDataApi = defaultApi) {
  /** Load the workflow picker and the triggers for current pipeline members. */
  async function loadPipelineData(memberIds: string[]): Promise<PipelineData> {
    const workflows = await api.fetchWorkflows();
    const memberSet = new Set(memberIds);
    const members = workflows.filter(
      (workflow): workflow is WorkflowDefinition & { id: string } =>
        workflow.id != null && memberSet.has(workflow.id),
    );
    const triggerLists = await Promise.all(
      members.map((workflow) => api.fetchWorkflowTriggers(workflow.id)),
    );
    const triggersByWorkflowId: Record<string, WorkflowTrigger[]> = {};
    members.forEach((workflow, index) => {
      triggersByWorkflowId[workflow.id] = triggerLists[index];
    });
    return { workflows, triggersByWorkflowId };
  }

  return { loadPipelineData };
}

export const { loadPipelineData } = createPipelineDataService();
