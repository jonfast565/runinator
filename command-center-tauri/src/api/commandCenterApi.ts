import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./tauriRuntime";
import type {
  JsonRecord,
  CredentialSummary,
  ProviderMetadata,
  RunArtifact,
  RunChunk,
  RunSummary,
  SaveTaskResponse,
  ScheduledTask,
  ServiceStatus,
  TaskResponse,
  WorkflowBundleSaveRequest,
  WorkflowBundleSaveResponse,
  WorkflowDefinition,
  WorkflowRunCreated,
  WorkflowRunDetail,
  WorkflowTrigger
} from "../types/models";

function command<T>(name: string, args?: Record<string, unknown>) {
  if (!isTauriRuntime()) {
    return Promise.reject(new Error("Tauri runtime unavailable. Open the app with `pnpm --dir command-center-tauri tauri dev` for live Runinator data."));
  }
  return invoke<T>(name, args);
}

export async function getServiceStatus() {
  return command<ServiceStatus>("get_service_status");
}

export async function startServiceDiscovery() {
  return command("start_service_discovery");
}

export async function fetchTasks() {
  return command<ScheduledTask[]>("fetch_tasks");
}

export async function saveTask(task: ScheduledTask, creating: boolean) {
  return command<SaveTaskResponse>("save_task", { request: { task, creating } });
}

export async function deleteTask(taskId: number) {
  return command<TaskResponse>("delete_task", { taskId });
}

export async function requestTaskRun(taskId: number) {
  return command<any>("request_task_run", { taskId });
}

export async function fetchTaskRuns(taskId: number) {
  return command<RunSummary[]>("fetch_task_runs", { taskId });
}

export async function fetchRunChunks(runId: number) {
  return command<RunChunk[]>("fetch_run_chunks", { runId });
}

export async function fetchRunArtifacts(runId: number) {
  return command<RunArtifact[]>("fetch_run_artifacts", { runId });
}

export async function fetchWorkflows() {
  return command<WorkflowDefinition[]>("fetch_workflows");
}

export async function saveWorkflow(workflow: WorkflowDefinition) {
  return command<WorkflowDefinition>("save_workflow", { workflow });
}

export async function saveWorkflowBundle(request: WorkflowBundleSaveRequest) {
  return command<WorkflowBundleSaveResponse>("save_workflow_bundle", { request });
}

export async function deleteWorkflow(workflowId: number) {
  return command<TaskResponse>("delete_workflow", { workflowId });
}

export async function fetchWorkflowTriggers(workflowId: number) {
  return command<WorkflowTrigger[]>("fetch_workflow_triggers", { workflowId });
}

export async function saveWorkflowTrigger(trigger: WorkflowTrigger, creating: boolean) {
  return command<WorkflowTrigger>("save_workflow_trigger", { trigger, creating });
}

export async function deleteWorkflowTrigger(triggerId: number) {
  return command<TaskResponse>("delete_workflow_trigger", { triggerId });
}

export async function createWorkflowRun(workflowId: number, options: { debug?: boolean } = {}) {
  return command<WorkflowRunCreated>("create_workflow_run", { workflowId, debug: Boolean(options.debug) });
}

export async function fetchWorkflowRuns(workflowId?: number) {
  return command<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

export async function fetchWorkflowRun(workflowRunId: number) {
  return command<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId });
}

export async function stepWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("step_workflow_run", { workflowRunId });
}

export async function fetchResourceRecords(endpoint: string) {
  return command<JsonRecord[]>("fetch_resource_records", { endpoint });
}

export async function fetchProviders() {
  return command<ProviderMetadata[]>("fetch_providers");
}

export async function fetchCredentials() {
  return command<CredentialSummary[]>("fetch_credentials");
}

export async function saveCredential(scope: string, name: string, secret: string) {
  return command<any>("save_credential", { request: { scope, name, secret } });
}

export async function deleteCredential(scope: string, name: string) {
  return command<any>("delete_credential", { scope, name });
}

export async function approveApproval(approvalId: number) {
  return command<any>("approve_approval", { approvalId });
}

export async function rejectApproval(approvalId: number) {
  return command<any>("reject_approval", { approvalId });
}
