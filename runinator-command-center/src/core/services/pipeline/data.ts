import { fetchWorkflowTriggers, fetchWorkflows } from "../../api/commandCenterApi";
import type { WorkflowDefinition, WorkflowTrigger } from "../../domain/models";

export interface PipelineData {
  workflows: WorkflowDefinition[];
  triggersByWorkflowId: Record<string, WorkflowTrigger[]>;
}

/** Load the workflow picker and the triggers for current pipeline members. */
export async function loadPipelineData(memberIds: string[]): Promise<PipelineData> {
  const workflows = await fetchWorkflows();
  const memberSet = new Set(memberIds);
  const members = workflows.filter(
    (workflow): workflow is WorkflowDefinition & { id: string } =>
      workflow.id != null && memberSet.has(workflow.id),
  );
  const triggerLists = await Promise.all(
    members.map((workflow) => fetchWorkflowTriggers(workflow.id)),
  );
  const triggersByWorkflowId: Record<string, WorkflowTrigger[]> = {};
  members.forEach((workflow, index) => {
    triggersByWorkflowId[workflow.id] = triggerLists[index];
  });
  return { workflows, triggersByWorkflowId };
}
