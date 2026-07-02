export const WorkflowsViewManifest = {
  "screen": "WorkflowsView",
  "tab": "Workflows",
  "services": [
    "WorkflowCatalogService",
    "WorkflowEditorService",
    "AppService",
    "ProvidersService",
    "SecretsService"
  ],
  "streams": [],
  "components": [
    "WorkflowCanvas",
    "WorkflowToolbar",
    "WorkflowInspector"
  ],
  "actions": [
    "saveWorkflow",
    "importWorkflow",
    "createRun"
  ]
} as const;
