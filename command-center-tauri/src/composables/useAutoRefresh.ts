import { onBeforeUnmount } from "vue";
import { useAppStore } from "../stores/app";
import { useResourcesStore } from "../stores/resources";
import { useTasksStore } from "../stores/tasks";
import { useWorkflowsStore } from "../stores/workflows";

export function useAutoRefresh() {
  const app = useAppStore();
  const tasks = useTasksStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();

  const refreshTimer = window.setInterval(() => {
    if (!tasks.taskEditorOpen) {
      tasks.refreshTasks();
      if (app.activeTab === "Workflows" && !workflows.isDirty) workflows.refreshWorkflows();
      if (workflows.selectedWorkflowRunId > 0) workflows.fetchWorkflowRunDetail(workflows.selectedWorkflowRunId);
      if (app.activeTab === "Resources") resources.refreshResources();
    }
  }, 10000);

  onBeforeUnmount(() => {
    window.clearInterval(refreshTimer);
  });
}
