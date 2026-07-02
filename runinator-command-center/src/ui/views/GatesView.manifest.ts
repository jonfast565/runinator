export const GatesViewManifest = {
  "screen": "GatesView",
  "tab": "Gates",
  "services": [
    "GatesService",
    "AppService"
  ],
  "streams": [],
  "components": [
    "DataTable"
  ],
  "actions": [
    "refreshGates",
    "resolveSelected",
    "removeSelected"
  ]
} as const;
