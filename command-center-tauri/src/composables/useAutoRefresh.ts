import { onBeforeUnmount } from "vue";
import { useAppStore } from "../stores/app";
import { useResourcesStore } from "../stores/resources";
import { useWorkflowsStore } from "../stores/workflows";

export function useAutoRefresh() {
  const app = useAppStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();

  const refreshTimer = window.setInterval(() => {
    if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
    if (app.activeTab === "Runs") workflows.fetchRecentWorkflowRuns();
    if (workflows.selectedWorkflowRunId > 0) workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId);
    if (app.activeTab === "Resources") resources.refreshResources();
  }, 10000);

  onBeforeUnmount(() => {
    window.clearInterval(refreshTimer);
  });
}
