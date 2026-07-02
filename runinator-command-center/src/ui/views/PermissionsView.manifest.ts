export const PermissionsViewManifest = {
  "screen": "PermissionsView",
  "tab": "Permissions",
  "services": [
    "PermissionsService",
    "AppService"
  ],
  "streams": [],
  "components": [
    "DataTable"
  ],
  "actions": [
    "createUser",
    "createTeam",
    "createApiKey"
  ]
} as const;
