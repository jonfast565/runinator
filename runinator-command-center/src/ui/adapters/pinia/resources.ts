import { defineStore } from "pinia";
import { computed } from "vue";
import { resourcesService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const resources = resourcesService.resourceEndpoints;

export const useResourcesStore = defineStore("resources", () => {
  const state = mirrorServiceState(resourcesService);

  const filteredResourceRecords = computed(() => resourcesService.filteredResourceRecords());
  const canResolveApproval = computed(() => resourcesService.canResolveApproval());
  const canDeleteSelected = computed(() => resourcesService.canDeleteSelected());

  return {
    resources,
    selectedResourceEndpoint: computed({
      get: () => state.value.selectedResourceEndpoint,
      set: (endpoint: string) => { resourcesService.setSelectedResourceEndpoint(endpoint); },
    }),
    resourceRecords: computed({
      get: () => state.value.resourceRecords,
      set: (records) => { resourcesService.setResourceRecords(records); },
    }),
    selectedResourceRecord: computed({
      get: () => state.value.selectedResourceRecord,
      set: (record) => { resourcesService.setSelectedResourceRecord(record); },
    }),
    hideResolved: computed({
      get: () => state.value.hideResolved,
      set: (value: boolean) => { resourcesService.setHideResolved(value); },
    }),
    canResolveApproval,
    canDeleteSelected,
    isResolved: resourcesService.isResolved,
    filteredResourceRecords,
    refreshResources: () => resourcesService.refreshResources(),
    refreshResourcesFor: (endpoint: string) => resourcesService.refreshResourcesFor(endpoint),
    clearResources: () => { resourcesService.clearResources(); },
    handleApprovalAction: (approvalId: string, action: "approve" | "reject") =>
      resourcesService.handleApprovalAction(approvalId, action),
    resolveApproval: (action: "approve" | "reject") => resourcesService.resolveApproval(action),
    resolveWorkflowApproval: resourcesService.resolveWorkflowApproval.bind(resourcesService),
    deleteSelected: () =>
      resourcesService.deleteSelected({
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
    recordType: resourcesService.recordType,
    recordSummary: resourcesService.recordSummary,
    moveResourceSelection: (delta: number) => { resourcesService.moveResourceSelection(delta); },
  };
});
