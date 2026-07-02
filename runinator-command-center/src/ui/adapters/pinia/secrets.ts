import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  deleteCredential,
  fetchCredential,
  fetchCredentials,
  saveCredential,
} from "../../../api/commandCenterApi";
import type { CredentialSummary, SettingKind } from "../../../types/models";
import { secretKey } from "../../../utils/secrets";
import { useAppStore } from "./app";

export const useSecretsStore = defineStore("secrets", () => {
  const secrets = ref<CredentialSummary[]>([]);
  const configValues = ref<Record<string, string>>({});
  const selectedSecretKey = ref("");
  const draft = reactive<{
    scope: string;
    name: string;
    secret: string;
    kind: SettingKind;
  }>({
    scope: "",
    name: "",
    secret: "",
    kind: "secret",
  });
  const app = useAppStore();

  const selectedSecret = computed(
    () => secrets.value.find((secret) => secretKey(secret) === selectedSecretKey.value) ?? null,
  );
  const configEntries = computed(() =>
    secrets.value.filter((secret) => (secret.kind ?? "secret") === "config"),
  );
  const secretEntries = computed(() =>
    secrets.value.filter((secret) => (secret.kind ?? "secret") === "secret"),
  );
  const filteredSecrets = computed(() => {
    const query = app.normalizedSearch;

    if (!query) {
      return secrets.value;
    }

    return secrets.value.filter((secret) =>
      [secret.scope, secret.name].some((value) => value.toLowerCase().includes(query)),
    );
  });
  const scopes = computed(() =>
    Array.from(new Set(secrets.value.map((secret) => secret.scope))).sort(),
  );

  async function refreshSecrets() {
    secrets.value = await app
      .runOperation("Refreshing secrets", () => fetchCredentials())
      .catch(() => []);
    secrets.value.sort(
      (left, right) => left.scope.localeCompare(right.scope) || left.name.localeCompare(right.name),
    );

    if (
      selectedSecretKey.value &&
      !secrets.value.some((secret) => secretKey(secret) === selectedSecretKey.value)
    ) {
      selectedSecretKey.value = "";
    }

    if (!selectedSecretKey.value && secrets.value.length > 0) {
      selectSecret(secrets.value[0]);
    }
  }

  function clearSecrets() {
    secrets.value = [];
    configValues.value = {};
    selectedSecretKey.value = "";
    clearDraft();
  }

  async function loadConfigValue(setting: CredentialSummary) {
    if ((setting.kind ?? "secret") !== "config") {
      return;
    }

    const key = secretKey(setting);
    const detail = await app.runOperation("Loading config value", () =>
      fetchCredential(setting.scope, setting.name, "config"),
    );
    configValues.value = {
      ...configValues.value,
      [key]: formatConfigValue(detail.value ?? detail.secret),
    };
  }

  async function loadConfigValues(settings: CredentialSummary[]) {
    await Promise.all(
      settings
        .filter((setting) => (setting.kind ?? "secret") === "config")
        .map((setting) => loadConfigValue(setting)),
    );
  }

  async function saveDraft() {
    const scope = draft.scope.trim();
    const name = draft.name.trim();
    const kind = draft.kind;
    const label = kind === "config" ? "Config" : "Secret";

    if (!scope || !name || !draft.secret.trim()) {
      app.setError(`${label} scope, name, and value are required`);
      return;
    }

    // config values are json; secrets are sent as a plain string. the web service infers a
    // config slot's schema from its first value, so the client never sends one.
    let value: unknown = draft.secret;

    if (kind === "config") {
      try {
        value = JSON.parse(draft.secret);
      } catch {
        app.setError("Config value must be valid JSON");
        return;
      }
    }

    await app.runOperation(`Saving ${kind}`, () => saveCredential(scope, name, value, kind));
    draft.secret = "";
    selectedSecretKey.value = secretKey({ scope, name, kind });
    app.setStatus(`${label} saved: ${scope}/${name}`);
    await refreshSecrets();
  }

  async function deleteSelectedSecret() {
    const secret = selectedSecret.value;

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
    selectedSecretKey.value = "";
    clearDraft();
    await refreshSecrets();
  }

  function selectSecret(secret: CredentialSummary) {
    selectedSecretKey.value = secretKey(secret);
    draft.scope = secret.scope;
    draft.name = secret.name;
    draft.secret = "";
    draft.kind = secret.kind ?? "secret";
  }

  function clearDraft(kind: SettingKind = "secret") {
    draft.scope = "";
    draft.name = "";
    draft.secret = "";
    draft.kind = kind;
  }

  function moveSecretSelection(delta: number) {
    const list = filteredSecrets.value;

    if (list.length === 0) {
      return;
    }

    const current = list.findIndex((secret) => secretKey(secret) === selectedSecretKey.value);
    selectSecret(list[boundedIndex(current, delta, list.length)]);
  }

  function secretsForScopes(credentialScopes: string[]): CredentialSummary[] {
    if (credentialScopes.length === 0) {
      return secrets.value;
    }

    const allowed = new Set(credentialScopes);
    return secrets.value.filter((secret) => allowed.has(secret.scope));
  }

  return {
    secrets,
    configValues,
    selectedSecretKey,
    draft,
    selectedSecret,
    configEntries,
    secretEntries,
    filteredSecrets,
    scopes,
    refreshSecrets,
    clearSecrets,
    loadConfigValue,
    loadConfigValues,
    saveDraft,
    deleteSelectedSecret,
    selectSecret,
    clearDraft,
    moveSecretSelection,
    secretsForScopes,
  };
});

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
