export const SecretsViewManifest = {
  "screen": "SecretsView",
  "tab": "Secrets",
  "services": [
    "SecretsService",
    "AppService"
  ],
  "streams": [],
  "components": [
    "SettingsTreeNode",
    "TypedValueEditor"
  ],
  "actions": [
    "refreshSecrets",
    "saveDraft",
    "deleteSelectedSecret"
  ]
} as const;
