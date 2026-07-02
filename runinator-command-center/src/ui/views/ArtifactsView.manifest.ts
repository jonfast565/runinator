export const ArtifactsViewManifest = {
  "screen": "ArtifactsView",
  "tab": "Artifacts",
  "services": [
    "ArtifactsService",
    "AppService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "DataTable"
  ],
  "actions": [
    "refreshArtifacts",
    "uploadArtifact",
    "downloadArtifact"
  ]
} as const;
