import { defineStore } from "pinia";
import { computed, reactive } from "vue";
import type { CredentialSummary, SettingKind } from "../../../core/domain/models";
import {
  blankSecretDraft,
  type SecretDraft,
} from "../../../core/services/secrets";
import { appService, secretsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export type { SecretDraft };

function blankDraft(kind: SettingKind = "secret"): SecretDraft {
  return blankSecretDraft(kind);
}

export const useSecretsStore = defineStore("secrets", () => {
  const state = mirrorServiceState(secretsService);
  const draft = reactive<SecretDraft>(blankDraft());

  return {
    secrets: computed({
      get: () => state.value.secrets,
      set: (secrets) => {
        secretsService.setState((current) => ({ ...current, secrets }));
      },
    }),
    configValues: computed(() => state.value.configValues),
    selectedSecretKey: computed({
      get: () => state.value.selectedSecretKey,
      set: (key) => { secretsService.setSelectedSecretKey(key); },
    }),
    draft,
    selectedSecret: computed(() => secretsService.selectedSecret()),
    configEntries: computed(() => secretsService.configEntries()),
    secretEntries: computed(() => secretsService.secretEntries()),
    filteredSecrets: computed(() => secretsService.filteredSecrets(appService.normalizedSearch)),
    scopes: computed(() => secretsService.scopes()),
    refreshSecrets: () => secretsService.refreshSecrets(),
    clearSecrets: () => {
      secretsService.clearSecrets();
      Object.assign(draft, blankDraft());
    },
    loadConfigValue: (setting: CredentialSummary) => secretsService.loadConfigValue(setting),
    loadConfigValues: (settings: CredentialSummary[]) => secretsService.loadConfigValues(settings),
    saveDraft: async () => {
      const saved = await secretsService.saveDraft({ ...draft });

      if (saved) {
        draft.secret = "";
      }
    },
    deleteSelectedSecret: async () => {
      await secretsService.deleteSelectedSecret();
      Object.assign(draft, blankDraft());
    },
    selectSecret: (secret: CredentialSummary) => {
      secretsService.selectSecret(secret);
      draft.scope = secret.scope;
      draft.name = secret.name;
      draft.secret = "";
      draft.kind = secret.kind ?? "secret";
    },
    clearDraft: (kind: SettingKind = "secret") => {
      secretsService.clearSelection();
      Object.assign(draft, blankDraft(kind));
    },
    moveSecretSelection: (delta: number) =>
      { secretsService.moveSecretSelection(delta, appService.normalizedSearch); },
    secretsForScopes: (credentialScopes: string[]) =>
      secretsService.secretsForScopes(credentialScopes),
  };
});
