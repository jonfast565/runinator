import { computed } from "vue";
import { defineStore } from "pinia";
import type { Action, AuthSessionSummary } from "../../../core/domain/models";
import { profileSecurityService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useProfileSecurityStore = defineStore("profile-security", () => {
  const state = mirrorServiceState(profileSecurityService);
  return {
    sessions: computed(() => state.value.sessions),
    apiKeys: computed(() => state.value.apiKeys),
    keyScopes: computed(() => state.value.keyScopes),
    revealedApiKey: computed(() => state.value.revealedApiKey),
    refresh: () => profileSecurityService.refresh(),
    updateEmail: (email: string | null) => profileSecurityService.updateEmail(email),
    changePassword: (currentPassword: string, newPassword: string) =>
      profileSecurityService.changePassword(currentPassword, newPassword),
    revokeSession: (session: AuthSessionSummary) => profileSecurityService.revokeSession(session),
    revokeOthers: () => profileSecurityService.revokeOthers(),
    createKey: (input: {
      name: string;
      orgId: string | null;
      expiresAt: string | null;
      actionCeiling: Action[];
    }) => profileSecurityService.createKey(input),
    updateKey: (keyId: string, name: string, expiresAt: string | null, disabled: boolean) =>
      profileSecurityService.updateKey(keyId, name, expiresAt, disabled),
    rotateKey: (keyId: string) => profileSecurityService.rotateKey(keyId),
    revokeKey: (keyId: string) => profileSecurityService.revokeKey(keyId),
    clearRevealedKey: () => {
      profileSecurityService.clearRevealedKey();
    },
  };
});
