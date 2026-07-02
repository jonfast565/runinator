export const DevViewManifest = {
  "screen": "DevView",
  "tab": "Dev",
  "services": [
    "AppService",
    "LocalWorkerService"
  ],
  "streams": [],
  "components": [
    "PackDiff",
    "WdlEditor"
  ],
  "actions": [
    "inspectDevPack",
    "applyDevPack"
  ]
} as const;
