export const ApprovalsViewManifest = {
  "screen": "ApprovalsView",
  "tab": "Approvals",
  "services": [
    "ResourcesService",
    "AppService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "ResourcesView",
    "DataTable"
  ],
  "actions": [
    "resolveApproval"
  ]
} as const;
