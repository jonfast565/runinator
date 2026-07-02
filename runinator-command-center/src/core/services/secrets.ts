import {
  deleteCredential,
  fetchCredential,
  fetchCredentials,
  saveCredential,
} from "../api/commandCenterApi";
import type { CredentialSummary, SettingKind } from "../domain/models";
import { secretKey } from "../utils/secrets";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

export interface SecretDraft {
  scope: string;
  name: string;
  secret: string;
  kind: SettingKind;
}

export interface SecretsState {
  secrets: CredentialSummary[];
  configValues: Record<string, string>;
  selectedSecretKey: string;
}

export function blankSecretDraft(kind: SettingKind = "secret"): SecretDraft {
  return {
    scope: "",
    name: "",
    secret: "",
    kind,
  };
}

export function createSecretsService(app: AppService) {
  const store = createStore<SecretsState>({
    secrets: [],
    configValues: {},
    selectedSecretKey: "",
  });

  function selectedSecret(): CredentialSummary | null {
    const { secrets, selectedSecretKey } = store.getState();
    return secrets.find((secret) => secretKey(secret) === selectedSecretKey) ?? null;
  }

  function configEntries(): CredentialSummary[] {
    return store.getState().secrets.filter((secret) => (secret.kind ?? "secret") === "config");
  }

  function secretEntries(): CredentialSummary[] {
    return store.getState().secrets.filter((secret) => (secret.kind ?? "secret") === "secret");
  }

  function filteredSecrets(query: string): CredentialSummary[] {
    const { secrets } = store.getState();

    if (!query) {
      return secrets;
    }

    return secrets.filter((secret) =>
      [secret.scope, secret.name].some((value) => value.toLowerCase().includes(query)),
    );
  }

  function scopes(): string[] {
    return Array.from(new Set(store.getState().secrets.map((secret) => secret.scope))).sort();
  }

  function secretsForScopes(credentialScopes: string[]): CredentialSummary[] {
    const { secrets } = store.getState();

    if (credentialScopes.length === 0) {
      return secrets;
    }

    const allowed = new Set(credentialScopes);
    return secrets.filter((secret) => allowed.has(secret.scope));
  }

  function moveSecretSelection(delta: number, query: string) {
    const list = filteredSecrets(query);

    if (list.length === 0) {
      return;
    }

    const { selectedSecretKey } = store.getState();
    const current = list.findIndex((secret) => secretKey(secret) === selectedSecretKey);
    service.selectSecret(list[boundedIndex(current, delta, list.length)]);
  }

  const service = {
    ...store,
    selectedSecret,
    configEntries,
    secretEntries,
    filteredSecrets,
    scopes,
    secretsForScopes,
    moveSecretSelection,
    setSelectedSecretKey(key: string) {
      store.setState((state) => ({ ...state, selectedSecretKey: key }));
    },
    selectSecret(secret: CredentialSummary) {
      store.setState((state) => ({ ...state, selectedSecretKey: secretKey(secret) }));
    },
    clearSelection() {
      store.setState((state) => ({ ...state, selectedSecretKey: "" }));
    },
    async refreshSecrets() {
      let secrets = await app
        .runOperation("Refreshing secrets", () => fetchCredentials())
        .catch(() => []);
      secrets = [...secrets].sort(
        (left, right) => left.scope.localeCompare(right.scope) || left.name.localeCompare(right.name),
      );

      let selectedSecretKey = store.getState().selectedSecretKey;

      if (selectedSecretKey && !secrets.some((secret) => secretKey(secret) === selectedSecretKey)) {
        selectedSecretKey = "";
      }

      if (!selectedSecretKey && secrets.length > 0) {
        selectedSecretKey = secretKey(secrets[0]);
      }

      store.setState((state) => ({
        ...state,
        secrets,
        selectedSecretKey,
      }));
    },
    clearSecrets() {
      store.setState(() => ({
        secrets: [],
        configValues: {},
        selectedSecretKey: "",
      }));
    },
    async loadConfigValue(setting: CredentialSummary) {
      if ((setting.kind ?? "secret") !== "config") {
        return;
      }

      const key = secretKey(setting);
      const detail = await app.runOperation("Loading config value", () =>
        fetchCredential(setting.scope, setting.name, "config"),
      );
      store.setState((state) => ({
        ...state,
        configValues: {
          ...state.configValues,
          [key]: formatConfigValue(detail.value ?? detail.secret),
        },
      }));
    },
    async loadConfigValues(settings: CredentialSummary[]) {
      await Promise.all(
        settings
          .filter((setting) => (setting.kind ?? "secret") === "config")
          .map((setting) => service.loadConfigValue(setting)),
      );
    },
    async saveDraft(draft: SecretDraft) {
      const scope = draft.scope.trim();
      const name = draft.name.trim();
      const kind = draft.kind;
      const label = kind === "config" ? "Config" : "Secret";

      if (!scope || !name || !draft.secret.trim()) {
        app.setError(`${label} scope, name, and value are required`);
        return false;
      }

      let value: unknown = draft.secret;

      if (kind === "config") {
        try {
          value = JSON.parse(draft.secret);
        } catch {
          app.setError("Config value must be valid JSON");
          return false;
        }
      }

      await app.runOperation(`Saving ${kind}`, () => saveCredential(scope, name, value, kind));
      store.setState((state) => ({
        ...state,
        selectedSecretKey: secretKey({ scope, name, kind }),
      }));
      app.setStatus(`${label} saved: ${scope}/${name}`);
      await service.refreshSecrets();
      return true;
    },
    async deleteSelectedSecret() {
      const secret = selectedSecret();

      if (!secret) {
        app.setError("No setting selected");
        return;
      }

      const kind = secret.kind ?? "secret";
      await app.runOperation(`Deleting ${kind}`, () =>
        deleteCredential(secret.scope, secret.name, kind),
      );
      app.setStatus(
        `${kind === "config" ? "Config" : "Secret"} deleted: ${secret.scope}/${secret.name}`,
      );
      store.setState((state) => ({ ...state, selectedSecretKey: "" }));
      await service.refreshSecrets();
    },
  };

  return service;
}

export type SecretsService = ReturnType<typeof createSecretsService>;

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) {
    return delta > 0 ? 0 : length - 1;
  }

  return Math.min(length - 1, Math.max(0, current + delta));
}

function formatConfigValue(value: unknown): string {
  if (value === undefined) {
    return "";
  }

  return JSON.stringify(value, null, 2);
}
