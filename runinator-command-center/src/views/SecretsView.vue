<template>
  <section class="pane settings-pane">
    <div class="settings-shell panel">
      <header class="settings-header">
        <div>
          <h2>{{ pageTitle }}</h2>
          <p>{{ pageDescription }}</p>
        </div>
        <div class="btn-row">
          <button class="btn" @click="refreshPage">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
          <button class="btn btn-primary" @click="openNewSetting">
            <Icon name="plus" />
            <span>{{ newLabel }}</span>
          </button>
        </div>
      </header>

      <div class="settings-summary">
        <div>
          <span>{{ isConfig ? "Configs" : "Secrets" }}</span>
          <strong>{{ filteredEntries.length }}</strong>
        </div>
        <div>
          <span>Scopes</span>
          <strong>{{ knownScopes.length }}</strong>
        </div>
        <div>
          <span>Values</span>
          <strong>{{ isConfig ? "Visible" : "Hidden" }}</strong>
        </div>
      </div>

      <DataTable>
        <table>
          <thead>
            <tr>
              <th>Scope</th>
              <th>Name</th>
              <th>Reference</th>
              <th v-if="isConfig">Value</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="setting in filteredEntries"
              :key="secretKey(setting)"
              :class="{ selected: secrets.selectedSecretKey === secretKey(setting) }"
              @click="openEditSetting(setting)"
            >
              <td>{{ setting.scope }}</td>
              <td>{{ setting.name }}</td>
              <td><code>{{ settingRef(setting.kind, setting.scope, setting.name) }}</code></td>
              <td v-if="isConfig">
                <pre class="config-value">{{ secrets.configValues[secretKey(setting)] || "Loading..." }}</pre>
              </td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </div>

    <div v-if="editorOpen" class="modal-backdrop" @click.self="closeEditor">
      <form class="modal setting-modal" @submit.prevent="saveEditor">
        <header class="modal-header">
          <div>
            <h2>{{ formTitle }}</h2>
            <p>{{ hint }}</p>
          </div>
          <button class="btn btn-ghost" type="button" @click="closeEditor">
            <Icon name="x" />
          </button>
        </header>

        <div class="form-grid setting-form-grid">
          <label>
            <span>Scope</span>
            <input v-model="secrets.draft.scope" list="setting-scopes" placeholder="github" />
          </label>
          <label>
            <span>Name</span>
            <input v-model="secrets.draft.name" placeholder="token" />
          </label>
          <label class="setting-value-field">
            <span>{{ isConfig ? "Config Value (JSON)" : "Secret Value" }}</span>
            <JsonEditor
              v-if="isConfig"
              class="setting-config-json"
              :model-value="secrets.draft.secret"
              title=""
              @update:model-value="secrets.draft.secret = $event"
            />
            <textarea v-else v-model="secrets.draft.secret" :placeholder="valuePlaceholder" />
          </label>
        </div>
        <datalist id="setting-scopes">
          <option v-for="scope in knownScopes" :key="scope" :value="scope" />
        </datalist>
        <div class="setting-reference-card">
          <span>Reference</span>
          <code>{{ settingRef(secrets.draft.kind, secrets.draft.scope, secrets.draft.name) }}</code>
        </div>

        <div class="modal-actions">
          <button class="btn btn-danger" type="button" :disabled="!secrets.selectedSecret" @click="deleteEditorSetting">
            <Icon name="trash" />
            <span>Delete</span>
          </button>
          <button class="btn" type="button" @click="closeEditor">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>{{ saveLabel }}</span>
          </button>
        </div>
      </form>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import { useAppStore } from "../stores/app";
import { useProvidersStore } from "../stores/providers";
import { useSecretsStore } from "../stores/secrets";
import type { CredentialSummary, SettingKind } from "../types/models";
import { secretKey, settingRef } from "../utils/secrets";

const props = defineProps<{
  settingKind: SettingKind;
}>();

const app = useAppStore();
const secrets = useSecretsStore();
const providers = useProvidersStore();
const editorOpen = ref(false);

const isConfig = computed(() => props.settingKind === "config");
const baseEntries = computed(() => (isConfig.value ? secrets.configEntries : secrets.secretEntries));
const filteredEntries = computed(() => {
  const query = app.normalizedSearch;
  if (!query) return baseEntries.value;
  return baseEntries.value.filter((setting) =>
    [setting.scope, setting.name, settingRef(setting.kind, setting.scope, setting.name)]
      .join(" ")
      .toLowerCase()
      .includes(query)
  );
});
const pageTitle = computed(() => (isConfig.value ? "Configs" : "Secrets"));
const pageDescription = computed(() =>
  isConfig.value
    ? "Plain JSON values resolved by the web service and visible to admins."
    : "Encrypted values resolved at the worker. Secret values are never displayed after saving."
);
const newLabel = computed(() => (isConfig.value ? "New Config" : "New Secret"));
const formTitle = computed(() => `${secrets.selectedSecret ? "Update" : "Add"} ${isConfig.value ? "Config" : "Secret"}`);
const saveLabel = computed(() => (isConfig.value ? "Save Config" : "Save Secret"));
const valuePlaceholder = computed(() => (isConfig.value ? 'JSON value, e.g. "https://api.example.com" or { "url": "..." }' : "Paste the secret value to add or replace it."));
const hint = computed(() =>
  isConfig.value
    ? "Config values are visible JSON. The web service infers the slot schema from the first value; later writes must stay consistent with it."
    : "Secret values are write-only from this interface. Select an existing secret, enter a new value, and save to replace it."
);

const knownScopes = computed(() => {
  const scopes = new Set<string>(secrets.scopes);
  for (const provider of providers.providers) {
    for (const scope of provider.metadata.credential_scopes) scopes.add(scope);
  }
  if (secrets.draft.scope.trim()) scopes.add(secrets.draft.scope.trim());
  return Array.from(scopes).sort();
});

async function refreshPage() {
  await secrets.refreshSecrets();
  if (isConfig.value) await secrets.loadConfigValues(filteredEntries.value);
}

function openNewSetting() {
  secrets.selectedSecretKey = "";
  secrets.clearDraft(props.settingKind);
  editorOpen.value = true;
}

async function openEditSetting(setting: CredentialSummary) {
  secrets.selectSecret(setting);
  if (isConfig.value) {
    await secrets.loadConfigValue(setting);
    secrets.draft.secret = secrets.configValues[secretKey(setting)] ?? "";
  }
  editorOpen.value = true;
}

function closeEditor() {
  editorOpen.value = false;
}

async function saveEditor() {
  secrets.draft.kind = props.settingKind;
  await secrets.saveDraft();
  if (!app.errorText) {
    if (isConfig.value && secrets.selectedSecret) await secrets.loadConfigValue(secrets.selectedSecret);
    editorOpen.value = false;
  }
}

async function deleteEditorSetting() {
  const setting = secrets.selectedSecret;
  if (!setting) return;
  if (!window.confirm(`Delete ${isConfig.value ? "config" : "secret"} ${setting.scope}/${setting.name}?`)) return;
  await secrets.deleteSelectedSecret();
  if (!app.errorText) editorOpen.value = false;
}

watch(
  () => props.settingKind,
  () => {
    editorOpen.value = false;
    secrets.clearDraft(props.settingKind);
    void refreshPage();
  }
);

onMounted(() => {
  if (providers.providers.length === 0 && !providers.loading) providers.fetchProviders();
  void refreshPage();
});
</script>

<style scoped>
.settings-shell {
  display: grid;
  gap: 12px;
  height: 100%;
  min-height: 0;
  grid-template-rows: auto auto 1fr;
}

.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.settings-header h2,
.settings-header p,
.modal-header p {
  margin: 0;
}

.settings-header p,
.modal-header p {
  color: var(--text-muted);
  font-size: 12px;
}

.settings-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.settings-summary div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.settings-summary span {
  color: var(--text-muted);
  font-size: 12px;
}

.settings-summary strong {
  color: var(--text);
  font-size: 16px;
}

.setting-modal {
  width: min(860px, 100%);
}

.setting-form-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.setting-value-field {
  grid-column: 1 / -1;
}

.setting-config-json {
  min-height: 140px;
}

.setting-config-json :deep(.json-editor-container) {
  min-height: 96px;
}

.setting-modal textarea {
  min-height: 180px;
}

.setting-reference-card {
  display: grid;
  gap: 6px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.setting-reference-card span {
  color: var(--text-muted);
  font-size: 12px;
}

.setting-reference-card code,
.config-value {
  overflow-wrap: anywhere;
}

.config-value {
  max-width: 520px;
  max-height: 120px;
  overflow: auto;
  margin: 0;
  white-space: pre-wrap;
}

@media (max-width: 920px) {
  .settings-shell {
    overflow: auto;
  }

  .settings-summary,
  .setting-form-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
