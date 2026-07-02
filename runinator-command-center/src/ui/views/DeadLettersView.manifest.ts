export const DeadLettersViewManifest = {
  "screen": "DeadLettersView",
  "tab": "DeadLetters",
  "services": [
    "AppService",
    "OrgsService"
  ],
  "streams": [],
  "components": [
    "EmptyState"
  ],
  "actions": [
    "listDeadLetters"
  ]
} as const;
