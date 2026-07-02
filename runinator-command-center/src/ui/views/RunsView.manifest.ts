export const RunsViewManifest = {
  "screen": "RunsView",
  "tab": "Runs",
  "services": [
    "WorkflowRunService",
    "AppService"
  ],
  "streams": [
    "WorkflowRunStreamClient",
    "NodeRunLogStreamClient"
  ],
  "components": [
    "RunTable",
    "WorkflowRunDetail",
    "LogPanel",
    "RunTimeline"
  ],
  "actions": [
    "selectRun",
    "cancelRun",
    "retryRun",
    "approveNode"
  ]
} as const;
