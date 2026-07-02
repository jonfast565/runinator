export const ReplicasViewManifest = {
  "screen": "ReplicasView",
  "tab": "Replicas",
  "services": [
    "AppService",
    "LocalWorkerService"
  ],
  "streams": [],
  "components": [
    "Sparkline",
    "LocalWorkerPanel",
    "NodePoolsPanel"
  ],
  "actions": [
    "refreshReplicas"
  ]
} as const;
