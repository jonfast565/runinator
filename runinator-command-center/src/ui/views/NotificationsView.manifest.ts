export const NotificationsViewManifest = {
  "screen": "NotificationsView",
  "tab": "Notifications",
  "services": [
    "NotificationsService",
    "AppService"
  ],
  "streams": [
    "EventStreamClient"
  ],
  "components": [
    "DataTable"
  ],
  "actions": [
    "markRead",
    "markAllRead",
    "deleteNotification"
  ]
} as const;
