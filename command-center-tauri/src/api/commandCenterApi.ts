import { invoke } from "@tauri-apps/api/core";
import type {
  JsonRecord,
  RunArtifact,
  RunChunk,
  RunSummary,
  SaveTaskResponse,
  ScheduledTask,
  ServiceStatus,
  TaskResponse,
  WorkflowDefinition,
  WorkflowRunCreated,
  WorkflowRunDetail
} from "../types/models";

export async function getServiceStatus() {
  return invoke<ServiceStatus>("get_service_status");
}

export async function startServiceDiscovery() {
  return invoke("start_service_discovery");
}

export async function fetchTasks() {
  return invoke<ScheduledTask[]>("fetch_tasks");
}

export async function saveTask(task: ScheduledTask, creating: boolean) {
  return invoke<SaveTaskResponse>("save_task", { request: { task, creating } });
}

export async function requestTaskRun(taskId: number) {
  return invoke<TaskResponse>("request_task_run", { taskId });
}

export async function fetchTaskRuns(taskId: number) {
  return invoke<RunSummary[]>("fetch_task_runs", { taskId });
}

export async function fetchRunChunks(runId: number) {
  return invoke<RunChunk[]>("fetch_run_chunks", { runId });
}

export async function fetchRunArtifacts(runId: number) {
  return invoke<RunArtifact[]>("fetch_run_artifacts", { runId });
}

export async function fetchWorkflows() {
  return invoke<WorkflowDefinition[]>("fetch_workflows");
}

export async function saveWorkflow(workflow: WorkflowDefinition) {
  return invoke<WorkflowDefinition>("save_workflow", { workflow });
}

export async function createWorkflowRun(workflowId: number) {
  return invoke<WorkflowRunCreated>("create_workflow_run", { workflowId });
}

export async function fetchWorkflowRuns(workflowId: number) {
  return invoke<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

export async function fetchWorkflowRun(workflowRunId: number) {
  return invoke<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId });
}

export async function fetchResourceRecords(endpoint: string) {
  return invoke<JsonRecord[]>("fetch_resource_records", { endpoint });
}

export async function approveApproval(approvalId: number) {
  return invoke<TaskResponse>("approve_approval", { approvalId });
}

export async function rejectApproval(approvalId: number) {
  return invoke<TaskResponse>("reject_approval", { approvalId });
}
