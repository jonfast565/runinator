export const AdminSettingsViewManifest = {
  "screen": "AdminSettingsView",
  "tab": "AdminSettings",
  "services": [
    "AdminSettingsService",
    "AppService"
  ],
  "streams": [],
  "components": [
    "SettingsTreeNode"
  ],
  "actions": [
    "refresh",
    "saveLanguage"
  ]
} as const;
