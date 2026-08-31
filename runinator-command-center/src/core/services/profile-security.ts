import {
  changeCurrentPassword,
  createPersonalApiKey,
  listCurrentSessions,
  listPersonalApiKeys,
  listPersonalApiKeyScopes,
  revokeApiKey,
  revokeCurrentSession,
  revokeOtherSessions,
  rotateApiKey,
  updateApiKey,
  updateCurrentUser,
} from "../api/commandCenterApi";
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

export function createProfileSecurityService(app: AppService, auth: AuthService) {
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
          Promise.all([listCurrentSessions(), listPersonalApiKeys(), listPersonalApiKeyScopes()]),
      );
      store.setState((state) => ({ ...state, sessions, apiKeys, keyScopes }));
    },
    async updateEmail(email: string | null) {
      await app.runOperation("Saving profile", () => updateCurrentUser({ email }));
      await auth.reloadMe();
      app.setStatus("Profile saved.");
    },
    async changePassword(currentPassword: string, newPassword: string) {
      await app.runOperation("Changing password", () =>
        changeCurrentPassword({
          current_password: currentPassword,
          new_password: newPassword,
        }),
      );
      await service.refresh();
      app.setStatus("Password changed. Other sessions were signed out.");
    },
    async revokeSession(session: AuthSessionSummary) {
      await app.runOperation("Signing out session", () => revokeCurrentSession(session.id));

      if (session.current) {
        await auth.signOut();
        return;
      }

      await service.refresh();
      app.setStatus("Session signed out.");
    },
    async revokeOthers() {
      await app.runOperation("Signing out other sessions", revokeOtherSessions);
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
        createPersonalApiKey({
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
        updateApiKey(keyId, { name, expires_at: expiresAt, disabled }),
      );
      await service.refreshKeys();
      app.setStatus("API key saved.");
    },
    async rotateKey(keyId: string) {
      const revealed = await app.runOperation("Rotating API key", () => rotateApiKey(keyId));
      store.setState((state) => ({ ...state, revealedApiKey: revealed }));
      await service.refreshKeys();
      app.setStatus("API key rotated. Copy the new secret now.");
    },
    async revokeKey(keyId: string) {
      await app.runOperation("Revoking API key", () => revokeApiKey(keyId));
      await service.refreshKeys();
      app.setStatus("API key revoked.");
    },
    async refreshKeys() {
      const apiKeys = await listPersonalApiKeys();
      store.setState((state) => ({ ...state, apiKeys }));
    },
    clearRevealedKey() {
      store.setState((state) => ({ ...state, revealedApiKey: null }));
    },
  };
  return service;
}

export type ProfileSecurityService = ReturnType<typeof createProfileSecurityService>;
