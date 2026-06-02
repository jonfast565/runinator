import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import { deleteCredential, fetchCredentials, saveCredential } from "../api/commandCenterApi";
import type { CredentialSummary, SettingKind } from "../types/models";
import { secretKey } from "../utils/secrets";
import { useAppStore } from "./app";

export const useSecretsStore = defineStore("secrets", () => {
  const secrets = ref<CredentialSummary[]>([]);
  const selectedSecretKey = ref("");
  const draft = reactive({
    scope: "",
    name: "",
    secret: "",
    kind: "secret" as SettingKind,
    // json-schema text for config values; required on the first write of a config slot.
    schema: ""
  });
  const app = useAppStore();

  const selectedSecret = computed(() => secrets.value.find((secret) => secretKey(secret) === selectedSecretKey.value) ?? null);
  const filteredSecrets = computed(() => {
    const query = app.normalizedSearch;
    if (!query) return secrets.value;
    return secrets.value.filter((secret) => [secret.scope, secret.name].some((value) => value.toLowerCase().includes(query)));
  });
  const scopes = computed(() => Array.from(new Set(secrets.value.map((secret) => secret.scope))).sort());

  async function refreshSecrets() {
    secrets.value = await app.runOperation("Refreshing secrets", () => fetchCredentials()).catch(() => []);
    secrets.value.sort((left, right) => left.scope.localeCompare(right.scope) || left.name.localeCompare(right.name));
    if (selectedSecretKey.value && !secrets.value.some((secret) => secretKey(secret) === selectedSecretKey.value)) {
      selectedSecretKey.value = "";
    }
    if (!selectedSecretKey.value && secrets.value.length > 0) {
      selectSecret(secrets.value[0]);
    }
  }

  function clearSecrets() {
    secrets.value = [];
    selectedSecretKey.value = "";
    clearDraft();
  }

  async function saveDraft() {
    const scope = draft.scope.trim();
    const name = draft.name.trim();
    const kind = draft.kind;
    const label = kind === "config" ? "Config" : "Secret";
    if (!scope || !name || !draft.secret) return app.setError(`${label} scope, name, and value are required`);

    // config values and schemas are json; secrets are sent as a plain string.
    let value: unknown = draft.secret;
    let schema: unknown;
    if (kind === "config") {
      try {
        value = JSON.parse(draft.secret);
      } catch {
        return app.setError("Config value must be valid JSON");
      }
      if (draft.schema.trim()) {
        try {
          schema = JSON.parse(draft.schema);
        } catch {
          return app.setError("Config schema must be valid JSON");
        }
      }
    }

    await app.runOperation(`Saving ${kind}`, () => saveCredential(scope, name, value, kind, schema));
    draft.secret = "";
    selectedSecretKey.value = secretKey({ scope, name, kind });
    app.setStatus(`${label} saved: ${scope}/${name}`);
    await refreshSecrets();
  }

  async function deleteSelectedSecret() {
    const secret = selectedSecret.value;
    if (!secret) return app.setError("No setting selected");
    const kind = secret.kind ?? "secret";
    await app.runOperation(`Deleting ${kind}`, () => deleteCredential(secret.scope, secret.name, kind));
    app.setStatus(`${kind === "config" ? "Config" : "Secret"} deleted: ${secret.scope}/${secret.name}`);
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
    draft.schema = "";
  }

  function clearDraft() {
    draft.scope = "";
    draft.name = "";
    draft.secret = "";
    draft.kind = "secret";
    draft.schema = "";
  }

  function moveSecretSelection(delta: number) {
    const list = filteredSecrets.value;
    if (list.length === 0) return;
    const current = list.findIndex((secret) => secretKey(secret) === selectedSecretKey.value);
    selectSecret(list[boundedIndex(current, delta, list.length)]);
  }

  function secretsForScopes(credentialScopes: string[]): CredentialSummary[] {
    if (credentialScopes.length === 0) return secrets.value;
    const allowed = new Set(credentialScopes);
    return secrets.value.filter((secret) => allowed.has(secret.scope));
  }

  return {
    secrets,
    selectedSecretKey,
    draft,
    selectedSecret,
    filteredSecrets,
    scopes,
    refreshSecrets,
    clearSecrets,
    saveDraft,
    deleteSelectedSecret,
    selectSecret,
    clearDraft,
    moveSecretSelection,
    secretsForScopes
  };
});

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}
