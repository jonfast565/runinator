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
  refreshNotifications: () => void;
  refreshSchedulesIfActive: () => void;
  refreshPipelineRunsIfActive: () => void;
  refreshPipelineDetailIfMember: (runId: string) => void;
}

export function createEventStreamRouter(deps: () => EventStreamRouterDeps): EventStreamRouter {
  return {
    route(event) {
      const context = deps();

      switch (event.type) {
        case "resync":
          context.refreshActiveState();
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
          // a member workflow run of the open pipeline-run detail just changed — refetch its steps.
          context.refreshPipelineDetailIfMember(runId);
          context.refreshResourcesIfActive();
          break;
        }

        case "pipeline_run_changed":
        case "pipeline_run_activity":
          context.refreshPipelineRunsIfActive();
          break;

        case "resources_changed":
          context.refreshResourcesIfActive();
          break;
        case "replicas_changed":
          context.refreshActiveState();
          break;
        case "notification_created":
        case "notifications_changed":
          context.refreshNotifications();
          break;
        case "schedules_changed":
          context.refreshSchedulesIfActive();
          break;
      }
    },
  };
}
