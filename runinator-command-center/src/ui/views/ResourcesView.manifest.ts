export const ResourcesViewManifest = {
  "screen": "ResourcesView",
  "tab": null,
  "services": [
    "ResourcesService",
    "AppService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "DataTable",
    "SplitPane",
    "MobileBackBar"
  ],
  "actions": [
    "refreshResources",
    "resolveApproval",
    "deleteSelected"
  ]
} as const;
