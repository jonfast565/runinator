export interface ServerEvent {
  type: string;
  [key: string]: unknown;
}

export interface EventStreamRouter {
  route(event: ServerEvent): void;
}

export interface EventStreamRouterDeps {
  activeTab: string;
  selectedWorkflowRunId: string | null;
  isWorkflowEditorDirty: boolean;
  refreshResourcesIfActive: () => void;
  refreshActiveState: () => void;
  refreshWorkflowsIfClean: () => void;
  refreshRecentRunsIfActive: () => void;
  refreshWorkflowRunIfSelected: (runId: string) => void;
  refreshArtifactsIfActive: () => void;
  refreshNotifications: () => void;
}

export function createEventStreamRouter(deps: () => EventStreamRouterDeps): EventStreamRouter {
  return {
    route(event) {
      const context = deps();

      switch (event.type) {
        case "run_status_changed":
          if (context.selectedWorkflowRunId) {
            context.refreshWorkflowRunIfSelected(context.selectedWorkflowRunId);
          }

          context.refreshResourcesIfActive();
          break;
        case "resync":
          context.refreshActiveState();
          break;
        case "tasks_changed":
          break;
        case "workflows_changed":
          context.refreshWorkflowsIfClean();
          break;

        case "workflow_run_changed": {
          const runId = event.run_id as string;

          if (context.selectedWorkflowRunId === runId) {
            context.refreshWorkflowRunIfSelected(runId);
          }

          context.refreshRecentRunsIfActive();
          context.refreshResourcesIfActive();
          break;
        }

        case "resources_changed":
          context.refreshResourcesIfActive();
          break;
        case "artifact_created":
        case "artifacts_changed":
          context.refreshArtifactsIfActive();
          break;
        case "notification_created":
        case "notifications_changed":
          context.refreshNotifications();
          break;
      }
    },
  };
}
