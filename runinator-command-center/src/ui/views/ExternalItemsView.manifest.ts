export const ExternalItemsViewManifest = {
  "screen": "ExternalItemsView",
  "tab": "ExternalItems",
  "services": [
    "ResourcesService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "ResourcesView"
  ],
  "actions": [
    "refreshResources"
  ]
} as const;
