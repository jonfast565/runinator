<template>
  <section class="pane secrets-pane">
    <SplitPane class="split" storage-key="command-center.secrets.split" :initial-first-pct="55" :min-first="360" :min-second="360">
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <div class="secrets-toolbar-copy">
              <h2>Config &amp; Secrets</h2>
              <p>Provider credentials and service config values available to workflows.</p>
            </div>
            <div class="btn-row">
              <button class="btn" @click="secrets.refreshSecrets">
                <Icon name="refresh" />
                <span>Refresh</span>
              </button>
              <button class="btn btn-danger" :disabled="!secrets.selectedSecret" @click="secrets.deleteSelectedSecret">
                <Icon name="trash" />
                <span>Remove</span>
              </button>
            </div>
          </div>
          <div class="secrets-summary">
            <div>
              <span>Entries</span>
              <strong>{{ secrets.filteredSecrets.length }}</strong>
            </div>
            <div>
              <span>Scopes</span>
              <strong>{{ knownScopes.length }}</strong>
            </div>
            <div>
              <span>Selected</span>
              <strong>{{ secrets.selectedSecret ? "1" : "0" }}</strong>
            </div>
          </div>
          <DataTable>
            <table>
              <thead>
                <tr>
                  <th>Kind</th>
                  <th>Scope</th>
                  <th>Name</th>
                  <th>Reference</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="secret in secrets.filteredSecrets"
                  :key="secretKey(secret)"
                  :class="{ selected: secrets.selectedSecretKey === secretKey(secret) }"
                  @click="secrets.selectSecret(secret)"
                >
                  <td>{{ secret.kind ?? "secret" }}</td>
                  <td>{{ secret.scope }}</td>
                  <td>{{ secret.name }}</td>
                  <td><code>{{ settingRef(secret.kind, secret.scope, secret.name) }}</code></td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
      <template #second>
        <form class="panel secret-form" @submit.prevent="secrets.saveDraft">
          <div class="panel-toolbar">
            <div class="secrets-toolbar-copy">
              <h2>{{ formTitle }}</h2>
              <p>{{ hint }}</p>
            </div>
            <div class="btn-row">
              <button class="btn" type="button" @click="secrets.clearDraft">
                <Icon name="x" />
                <span>Clear</span>
              </button>
              <button class="btn btn-primary" type="submit">
                <Icon name="save" />
                <span>{{ saveLabel }}</span>
              </button>
            </div>
          </div>
          <div class="secret-form-body">
            <fieldset class="kind-field">
              <legend>Kind</legend>
              <div class="kind-options">
                <label class="kind-option" :class="{ active: secrets.draft.kind === 'secret' }">
                  <input type="radio" name="setting-kind" value="secret" v-model="secrets.draft.kind" />
                  <span class="kind-option-body">
                    <span class="kind-option-title">Secret</span>
                    <span class="kind-option-desc">Encrypted, resolved at the worker</span>
                  </span>
                </label>
                <label class="kind-option" :class="{ active: secrets.draft.kind === 'config' }">
                  <input type="radio" name="setting-kind" value="config" v-model="secrets.draft.kind" />
                  <span class="kind-option-body">
                    <span class="kind-option-title">Config</span>
                    <span class="kind-option-desc">Plain JSON, resolved by the web service</span>
                  </span>
                </label>
              </div>
            </fieldset>
            <div class="form-grid secret-form-grid">
              <label>
                <span>Scope</span>
                <input v-model="secrets.draft.scope" list="secret-scopes" placeholder="github" />
              </label>
              <label>
                <span>Name</span>
                <input v-model="secrets.draft.name" placeholder="token" />
              </label>
              <label class="secret-value-field">
                <span>{{ isConfig ? "Config Value (JSON)" : "Secret Value" }}</span>
                <textarea v-model="secrets.draft.secret" :placeholder="valuePlaceholder" />
              </label>
            </div>
            <datalist id="secret-scopes">
              <option v-for="scope in knownScopes" :key="scope" :value="scope" />
            </datalist>
            <div class="secret-reference-card">
              <span>Reference</span>
              <code>{{ settingRef(secrets.draft.kind, secrets.draft.scope, secrets.draft.name) }}</code>
            </div>
          </div>
        </form>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useProvidersStore } from "../stores/providers";
import { useSecretsStore } from "../stores/secrets";
import { secretKey, settingRef } from "../utils/secrets";

const secrets = useSecretsStore();
const providers = useProvidersStore();

const isConfig = computed(() => secrets.draft.kind === "config");
const formTitle = computed(() => `${secrets.selectedSecret ? "Update" : "Add"} ${isConfig.value ? "Config" : "Secret"}`);
const saveLabel = computed(() => (isConfig.value ? "Save Config" : "Save Secret"));
const valuePlaceholder = computed(() =>
  isConfig.value ? 'JSON value, e.g. "https://api.example.com" or { "url": "..." }' : "Paste the secret value to add or replace it."
);
const hint = computed(() =>
  isConfig.value
    ? "Config values are plain JSON read by the web service. The schema is inferred from the first value; later writes must stay consistent with it. Reference them in WDL as config.scope.name."
    : "Secret values are not shown after saving. Select an existing secret, enter a new value, and save to replace it."
);

const knownScopes = computed(() => {
  const scopes = new Set<string>(secrets.scopes);
  for (const provider of providers.providers) {
    for (const scope of provider.metadata.credential_scopes) scopes.add(scope);
  }
  if (secrets.draft.scope.trim()) scopes.add(secrets.draft.scope.trim());
  return Array.from(scopes).sort();
});

onMounted(() => {
  if (providers.providers.length === 0 && !providers.loading) providers.fetchProviders();
  secrets.refreshSecrets();
});
</script>

<style scoped>
.secret-form {
  overflow: auto;
}

.secrets-toolbar-copy {
  display: grid;
  gap: 4px;
}

.secrets-toolbar-copy p {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.secrets-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.secrets-summary div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.secrets-summary span {
  color: var(--text-muted);
  font-size: 12px;
}

.secrets-summary strong {
  color: var(--text);
  font-size: 16px;
}

.secret-form-body {
  display: grid;
  gap: 14px;
}

.secret-form-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.secret-value-field {
  grid-column: 1 / -1;
}

.secret-form textarea {
  min-height: 180px;
}

.kind-field {
  border: 0;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 8px;
}

.kind-field legend {
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
  padding: 0;
}

.kind-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.kind-option {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface);
  padding: 12px;
  cursor: pointer;
  transition: border-color 0.12s ease, background 0.12s ease, box-shadow 0.12s ease;
}

.kind-option:hover {
  border-color: var(--border-strong);
  background: var(--surface-hover);
}

.kind-option.active {
  border-color: var(--accent);
  background: var(--accent-soft);
  box-shadow: 0 0 0 1px var(--accent) inset;
}

.kind-option input {
  margin: 2px 0 0;
  accent-color: var(--accent);
  flex: 0 0 auto;
}

.kind-option-body {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.kind-option-title {
  color: var(--text);
  font-size: 13px;
  font-weight: 600;
}

.kind-option-desc {
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.35;
}

.secret-reference-card {
  display: grid;
  gap: 6px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 10px 12px;
}

.secret-reference-card span {
  color: var(--text-muted);
  font-size: 12px;
}

.secret-reference-card code {
  overflow-wrap: anywhere;
  color: var(--accent-text);
}

@media (max-width: 980px) {
  .secrets-summary,
  .secret-form-grid,
  .kind-options {
    grid-template-columns: 1fr;
  }
}
</style>
