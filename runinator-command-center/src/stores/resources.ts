import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  approveApproval,
  deleteAutomationEvent,
  fetchResourceRecords,
  rejectApproval
} from "../api/commandCenterApi";
import type { ResourceEndpoint } from "../types/app";
import type { JsonRecord, WorkflowNodeRun } from "../types/models";
import { approvalIdFromNodeRun, type ApprovalAction, nonEmptyString, selectWorkflowApprovalRecord } from "../utils/approvals";
import { genericRecordSummary, genericRecordType } from "../utils/resources";
import { useAppStore } from "./app";

export const resources: ResourceEndpoint[] = [
  { label: "External Items", endpoint: "external_items" },
  { label: "Approvals", endpoint: "approvals" },
  { label: "Events", endpoint: "automation_events" }
];

export const useResourcesStore = defineStore("resources", () => {
  const selectedResourceEndpoint = ref("external_items");
  const resourceRecords = ref<JsonRecord[]>([]);
  const selectedResourceRecord = ref<JsonRecord | null>(null);
  // when true, resolved approvals are hidden from the list entirely.
  const hideResolved = ref(false);
  const app = useAppStore();

  // an approval (or gate-like record) is resolved once it has a resolution timestamp or a terminal
  // status; resolved rows are greyed out and can no longer be approved/rejected.
  function isResolved(record: JsonRecord | null | undefined): boolean {
    if (!record) return false;
    if (nonEmptyString(record.resolved_at)) return true;
    const status = String(record.status ?? "").toLowerCase();
    return ["approved", "rejected", "resolved", "cancelled", "canceled", "expired"].includes(status);
  }

  const canResolveApproval = computed(
    () =>
      selectedResourceEndpoint.value === "approvals" &&
      Boolean(nonEmptyString(selectedResourceRecord.value?.id)) &&
      !isResolved(selectedResourceRecord.value)
  );
  const filteredResourceRecords = computed(() => {
    const query = app.normalizedSearch;
    let records = resourceRecords.value;
    if (hideResolved.value && selectedResourceEndpoint.value === "approvals") {
      records = records.filter((record) => !isResolved(record));
    }
    if (!query) return records;
    return records.filter((record) =>
      [record.id, record.provider, recordType(record), record.status, recordSummary(record), record.external_id, record.key, record.url]
        .filter((value) => value !== undefined && value !== null)
        .some((value) => String(value).toLowerCase().includes(query))
    );
  });

  async function refreshResources() {
    resourceRecords.value = await app.runOperation("Refreshing resources", () => fetchResourceRecords(selectedResourceEndpoint.value)).catch(() => []);
    selectedResourceRecord.value = resourceRecords.value[0] ?? null;
  }

  async function refreshResourcesFor(endpoint: string) {
    if (selectedResourceEndpoint.value !== endpoint) {
      selectedResourceEndpoint.value = endpoint;
      selectedResourceRecord.value = null;
    }
    await refreshResources();
  }

  function clearResources() {
    resourceRecords.value = [];
    selectedResourceRecord.value = null;
  }

  async function handleApprovalAction(approvalId: string, action: ApprovalAction) {
    const response = await app.runOperation(`${action === "approve" ? "Approving" : "Rejecting"} approval`, () =>
      action === "approve" ? approveApproval(approvalId) : rejectApproval(approvalId)
    );
    app.setStatus(response.message || `Approval ${action === "approve" ? "approved" : "rejected"}`);
    await refreshResources();
  }

  async function resolveApproval(action: ApprovalAction) {
    if (!canResolveApproval.value) return app.setError("No approval selected");
    const approvalId = nonEmptyString(selectedResourceRecord.value?.id);
    if (!approvalId) return app.setError("No approval selected");
    await handleApprovalAction(approvalId, action);
  }

  async function resolveWorkflowApproval(workflowRunId: string, nodeId: string, nodeRun: WorkflowNodeRun, action: ApprovalAction) {
    const approvalId = await findWorkflowApprovalId(workflowRunId, nodeId, nodeRun);
    if (!approvalId) return;
    await handleApprovalAction(approvalId, action);
  }

  async function findWorkflowApprovalId(workflowRunId: string, nodeId: string, nodeRun: WorkflowNodeRun): Promise<string | null> {
    const stateApprovalId = approvalIdFromNodeRun(nodeRun);
    if (stateApprovalId) return stateApprovalId;

    const approvals = await app.runOperation("Loading workflow approvals", () => fetchResourceRecords(`approvals?workflow_run_id=${workflowRunId}`));
    const approval = selectWorkflowApprovalRecord(approvals, workflowRunId, nodeId);
    const approvalId = nonEmptyString(approval?.id);
    if (approvalId) return approvalId;

    app.setError(`No approval found for workflow node ${nodeId}`);
    return null;
  }

  const canDeleteSelected = computed(
    () =>
      selectedResourceEndpoint.value === "automation_events" &&
      Boolean(nonEmptyString(selectedResourceRecord.value?.id))
  );

  async function deleteSelected() {
    const id = nonEmptyString(selectedResourceRecord.value?.id);
    if (!id) return app.setError("No record selected");
    if (selectedResourceEndpoint.value !== "automation_events") return;
    if (!window.confirm("Delete this event record?")) return;
    await app.runOperation("Deleting event", () => deleteAutomationEvent(id)).catch((error) => {
      app.setError(String(error));
    });
    resourceRecords.value = resourceRecords.value.filter((record) => record.id !== id);
    selectedResourceRecord.value = resourceRecords.value[0] ?? null;
    await refreshResources();
  }

  function recordType(record: JsonRecord) {
    return genericRecordType(record, selectedResourceEndpoint.value);
  }

  function recordSummary(record: JsonRecord) {
    return genericRecordSummary(record);
  }

  function moveResourceSelection(delta: number) {
    const list = filteredResourceRecords.value;
    if (list.length === 0) return;
    const current = list.findIndex((record) => record === selectedResourceRecord.value);
    selectedResourceRecord.value = list[boundedIndex(current, delta, list.length)];
  }

  return {
    resources,
    selectedResourceEndpoint,
    resourceRecords,
    selectedResourceRecord,
    hideResolved,
    canResolveApproval,
    canDeleteSelected,
    isResolved,
    filteredResourceRecords,
    refreshResources,
    refreshResourcesFor,
    clearResources,
    handleApprovalAction,
    resolveApproval,
    resolveWorkflowApproval,
    deleteSelected,
    recordType,
    recordSummary,
    moveResourceSelection
  };
});

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}
