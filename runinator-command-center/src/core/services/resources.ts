import {
  approveApproval,
  deleteAutomationEvent,
  fetchResourceRecords,
  rejectApproval,
} from "../api/commandCenterApi";
import type { ResourceEndpoint } from "../navigation/app";
import type { JsonRecord, WorkflowNodeRun } from "../domain/models";
import {
  approvalIdFromNodeRun,
  type ApprovalAction,
  nonEmptyString,
  selectWorkflowApprovalRecord,
} from "../utils/approvals";
import { genericRecordSummary, genericRecordType } from "../utils/resources";
import { displayValue } from "../utils/values";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext, OperationContext } from "./operation-context";

export const resourceEndpoints: ResourceEndpoint[] = [
  { label: "External Items", endpoint: "external_items" },
  { label: "Approvals", endpoint: "approvals" },
  { label: "Events", endpoint: "automation_events" },
];

export interface ResourcesState {
  selectedResourceEndpoint: string;
  resourceRecords: JsonRecord[];
  selectedResourceRecord: JsonRecord | null;
  hideResolved: boolean;
}

export function createResourcesService(app: AppService) {
  const store = createStore<ResourcesState>({
    selectedResourceEndpoint: "external_items",
    resourceRecords: [],
    selectedResourceRecord: null,
    hideResolved: false,
  });

  function operationContext(): OperationContext {
    return {
      runOperation: (label, operation) => app.runOperation(label, operation),
      setStatus: (text) => { app.setStatus(text); },
      setError: (text) => { app.setError(text); },
      normalizedSearch: app.normalizedSearch,
    };
  }

  function isResolved(record: JsonRecord | null | undefined): boolean {
    if (!record) {
      return false;
    }

    if (nonEmptyString(record.resolved_at)) {
      return true;
    }

    const status = displayValue(record.status).toLowerCase();
    return ["approved", "rejected", "resolved", "cancelled", "canceled", "expired"].includes(
      status,
    );
  }

  function recordType(record: JsonRecord) {
    return genericRecordType(record, store.getState().selectedResourceEndpoint);
  }

  function recordSummary(record: JsonRecord) {
    return genericRecordSummary(record);
  }

  function filteredResourceRecords(): JsonRecord[] {
    const query = app.normalizedSearch;
    let records = store.getState().resourceRecords;

    if (store.getState().hideResolved && store.getState().selectedResourceEndpoint === "approvals") {
      records = records.filter((record) => !isResolved(record));
    }

    if (!query) {
      return records;
    }

    return records.filter((record) =>
      [
        record.id,
        record.provider,
        recordType(record),
        record.status,
        recordSummary(record),
        record.external_id,
        record.key,
        record.url,
      ]
        .filter((value) => value !== undefined && value !== null)
        .some((value) => displayValue(value).toLowerCase().includes(query)),
    );
  }

  const service = {
    ...store,
    resourceEndpoints,
    isResolved,
    filteredResourceRecords,
    recordType,
    recordSummary,
    canResolveApproval() {
      const state = store.getState();
      return (
        state.selectedResourceEndpoint === "approvals" &&
        Boolean(nonEmptyString(state.selectedResourceRecord?.id)) &&
        !isResolved(state.selectedResourceRecord)
      );
    },
    canDeleteSelected() {
      const state = store.getState();
      return (
        state.selectedResourceEndpoint === "automation_events" &&
        Boolean(nonEmptyString(state.selectedResourceRecord?.id))
      );
    },
    setHideResolved(value: boolean) {
      store.setState((state) => ({ ...state, hideResolved: value }));
    },
    setSelectedResourceEndpoint(endpoint: string) {
      store.setState((state) => ({ ...state, selectedResourceEndpoint: endpoint }));
    },
    setSelectedResourceRecord(record: JsonRecord | null) {
      store.setState((state) => ({ ...state, selectedResourceRecord: record }));
    },
    setResourceRecords(records: JsonRecord[]) {
      store.setState((state) => ({ ...state, resourceRecords: records }));
    },
    async refreshResources() {
      const endpoint = store.getState().selectedResourceEndpoint;
      const records = await operationContext()
        .runOperation("Refreshing resources", () => fetchResourceRecords(endpoint))
        .catch(() => []);
      store.setState((state) => ({
        ...state,
        resourceRecords: records,
        selectedResourceRecord: records[0] ?? null,
      }));
    },
    async refreshResourcesFor(endpoint: string) {
      if (store.getState().selectedResourceEndpoint !== endpoint) {
        store.setState((state) => ({
          ...state,
          selectedResourceEndpoint: endpoint,
          selectedResourceRecord: null,
        }));
      }

      await service.refreshResources();
    },
    clearResources() {
      store.setState((state) => ({
        ...state,
        resourceRecords: [],
        selectedResourceRecord: null,
      }));
    },
    async handleApprovalAction(approvalId: string, action: ApprovalAction) {
      const ctx = operationContext();
      const response = await ctx.runOperation(
        `${action === "approve" ? "Approving" : "Rejecting"} approval`,
        () => (action === "approve" ? approveApproval(approvalId) : rejectApproval(approvalId)),
      );
      ctx.setStatus(response.message || `Approval ${action === "approve" ? "approved" : "rejected"}`);
      await service.refreshResources();
    },
    async resolveApproval(action: ApprovalAction) {
      if (!service.canResolveApproval()) {
        app.setError("No approval selected");
        return;
      }

      const approvalId = nonEmptyString(store.getState().selectedResourceRecord?.id);

      if (!approvalId) {
        app.setError("No approval selected");
        return;
      }

      await service.handleApprovalAction(approvalId, action);
    },
    async resolveWorkflowApproval(
      workflowRunId: string,
      nodeId: string,
      nodeRun: WorkflowNodeRun,
      action: ApprovalAction,
    ) {
      const approvalId = await service.findWorkflowApprovalId(workflowRunId, nodeId, nodeRun);

      if (!approvalId) {
        return;
      }

      await service.handleApprovalAction(approvalId, action);
    },
    async findWorkflowApprovalId(
      workflowRunId: string,
      nodeId: string,
      nodeRun: WorkflowNodeRun,
    ): Promise<string | null> {
      const stateApprovalId = approvalIdFromNodeRun(nodeRun);

      if (stateApprovalId) {
        return stateApprovalId;
      }

      const ctx = operationContext();
      const approvals = await ctx.runOperation("Loading workflow approvals", () =>
        fetchResourceRecords(`approvals?workflow_run_id=${workflowRunId}`),
      );
      const approval = selectWorkflowApprovalRecord(approvals, workflowRunId, nodeId);
      const approvalId = nonEmptyString(approval?.id);

      if (approvalId) {
        return approvalId;
      }

      ctx.setError(`No approval found for workflow node ${nodeId}`);
      return null;
    },
    async deleteSelected(confirm: ConfirmContext) {
      const id = nonEmptyString(store.getState().selectedResourceRecord?.id);

      if (!id) {
        app.setError("No record selected");
        return;
      }

      if (store.getState().selectedResourceEndpoint !== "automation_events") {
        return;
      }

      if (!confirm.confirm("Delete this event record?")) {
        return;
      }

      await operationContext()
        .runOperation("Deleting event", () => deleteAutomationEvent(id))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      store.setState((state) => ({
        ...state,
        resourceRecords: state.resourceRecords.filter((record) => record.id !== id),
        selectedResourceRecord: state.resourceRecords[0] ?? null,
      }));
      await service.refreshResources();
    },
    moveResourceSelection(delta: number) {
      const list = filteredResourceRecords();

      if (list.length === 0) {
        return;
      }

      const current = list.findIndex((record) => record === store.getState().selectedResourceRecord);
      store.setState((state) => ({
        ...state,
        selectedResourceRecord: list[boundedIndex(current, delta, list.length)],
      }));
    },
  };

  return service;
}

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) {
    return delta > 0 ? 0 : length - 1;
  }

  return Math.min(length - 1, Math.max(0, current + delta));
}

export type ResourcesService = ReturnType<typeof createResourcesService>;
