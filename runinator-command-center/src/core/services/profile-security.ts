import { defaultApi, type ProfileSecurityApi } from "../api/ports/profile-security";

import type {
  Action,
  ApiKey,
  AuthSessionSummary,
  PersonalApiKeySecret,
  PersonalApiKeyScope,
} from "../domain/models";
import type { AppService } from "./app";
import type { AuthService } from "./auth";
import { createStore } from "./event-bus";

export interface ProfileSecurityState {
  sessions: AuthSessionSummary[];
  apiKeys: ApiKey[];
  keyScopes: PersonalApiKeyScope[];
  revealedApiKey: PersonalApiKeySecret | null;
}

export function createProfileSecurityService(
  app: AppService,
  auth: AuthService,
  api: ProfileSecurityApi = defaultApi,
) {
  const store = createStore<ProfileSecurityState>({
    sessions: [],
    apiKeys: [],
    keyScopes: [],
    revealedApiKey: null,
  });

  const service = {
    ...store,
    async refresh() {
      const [sessions, apiKeys, keyScopes] = await app.runOperation(
        "Loading profile security",
        () =>
          Promise.all([
            api.listCurrentSessions(),
            api.listPersonalApiKeys(),
            api.listPersonalApiKeyScopes(),
          ]),
      );
      store.setState((state) => ({ ...state, sessions, apiKeys, keyScopes }));
    },
    async updateEmail(email: string | null) {
      await app.runOperation("Saving profile", () => api.updateCurrentUser({ email }));
      await auth.reloadMe();
      app.setStatus("Profile saved.");
    },
    async changePassword(currentPassword: string, newPassword: string) {
      await app.runOperation("Changing password", () =>
        api.changeCurrentPassword({
          current_password: currentPassword,
          new_password: newPassword,
        }),
      );
      await service.refresh();
      app.setStatus("Password changed. Other sessions were signed out.");
    },
    async revokeSession(session: AuthSessionSummary) {
      await app.runOperation("Signing out session", () => api.revokeCurrentSession(session.id));

      if (session.current) {
        await auth.signOut();
        return;
      }

      await service.refresh();
      app.setStatus("Session signed out.");
    },
    async revokeOthers() {
      await app.runOperation("Signing out other sessions", api.revokeOtherSessions);
      await service.refresh();
      app.setStatus("Other sessions signed out.");
    },
    async createKey(input: {
      name: string;
      orgId: string | null;
      expiresAt: string | null;
      actionCeiling: Action[];
    }) {
      const revealed = await app.runOperation("Creating API key", () =>
        api.createPersonalApiKey({
          name: input.name,
          org_id: input.orgId,
          expires_at: input.expiresAt,
          action_ceiling: input.actionCeiling,
        }),
      );
      store.setState((state) => ({ ...state, revealedApiKey: revealed }));
      await service.refreshKeys();
      app.setStatus("API key created. Copy the secret now.");
    },
    async updateKey(keyId: string, name: string, expiresAt: string | null, disabled: boolean) {
      await app.runOperation("Updating API key", () =>
        api.updateApiKey(keyId, { name, expires_at: expiresAt, disabled }),
      );
      await service.refreshKeys();
      app.setStatus("API key saved.");
    },
    async rotateKey(keyId: string) {
      const revealed = await app.runOperation("Rotating API key", () => api.rotateApiKey(keyId));
      store.setState((state) => ({ ...state, revealedApiKey: revealed }));
      await service.refreshKeys();
      app.setStatus("API key rotated. Copy the new secret now.");
    },
    async revokeKey(keyId: string) {
      await app.runOperation("Revoking API key", () => api.revokeApiKey(keyId));
      await service.refreshKeys();
      app.setStatus("API key revoked.");
    },
    async refreshKeys() {
      const apiKeys = await api.listPersonalApiKeys();
      store.setState((state) => ({ ...state, apiKeys }));
    },
    clearRevealedKey() {
      store.setState((state) => ({ ...state, revealedApiKey: null }));
    },
  };
  return service;
}

export type ProfileSecurityService = ReturnType<typeof createProfileSecurityService>;
