export const EventsViewManifest = {
  "screen": "EventsView",
  "tab": "Events",
  "services": [
    "ResourcesService",
    "OrgsService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "DataTable",
    "SplitPane"
  ],
  "actions": [
    "deleteSelected"
  ]
} as const;
