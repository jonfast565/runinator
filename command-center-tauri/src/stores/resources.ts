import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { approveApproval, fetchResourceRecords, rejectApproval } from "../api/commandCenterApi";
import type { ResourceEndpoint } from "../types/app";
import type { JsonRecord } from "../types/models";
import { genericRecordSummary, genericRecordType } from "../utils/resources";
import { useAppStore } from "./app";

export const resources: ResourceEndpoint[] = [
  { label: "External Items", endpoint: "external_items" },
  { label: "Resources", endpoint: "external_resources" },
  { label: "Feedback", endpoint: "feedback" },
  { label: "Approvals", endpoint: "approvals" },
  { label: "Gates", endpoint: "gates" },
  { label: "Workspaces", endpoint: "workspaces" },
  { label: "Change Sets", endpoint: "change_sets" },
  { label: "Events", endpoint: "automation_events" }
];

export const useResourcesStore = defineStore("resources", () => {
  const selectedResourceEndpoint = ref("external_items");
  const resourceRecords = ref<JsonRecord[]>([]);
  const selectedResourceRecord = ref<JsonRecord | null>(null);
  const app = useAppStore();

  const canResolveApproval = computed(() => selectedResourceEndpoint.value === "approvals" && Number(selectedResourceRecord.value?.id ?? 0) > 0);
  const filteredResourceRecords = computed(() => {
    const query = app.normalizedSearch;
    if (!query) return resourceRecords.value;
    return resourceRecords.value.filter((record) =>
      [record.id, record.provider, recordType(record), record.status, recordSummary(record), record.external_id, record.key, record.url]
        .filter((value) => value !== undefined && value !== null)
        .some((value) => String(value).toLowerCase().includes(query))
    );
  });

  async function refreshResources() {
    resourceRecords.value = await app.runOperation("Refreshing resources", () => fetchResourceRecords(selectedResourceEndpoint.value)).catch(() => []);
    selectedResourceRecord.value = resourceRecords.value[0] ?? null;
  }

  async function resolveApproval(action: "approve" | "reject") {
    if (!canResolveApproval.value) return app.setError("No approval selected");
    const approvalId = Number(selectedResourceRecord.value?.id);
    const response = await app.runOperation(`${action === "approve" ? "Approving" : "Rejecting"} approval`, () =>
      action === "approve" ? approveApproval(approvalId) : rejectApproval(approvalId)
    );
    app.setStatus(response.message);
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
    canResolveApproval,
    filteredResourceRecords,
    refreshResources,
    resolveApproval,
    recordType,
    recordSummary,
    moveResourceSelection
  };
});

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}
