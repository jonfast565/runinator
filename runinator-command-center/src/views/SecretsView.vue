<template>
  <section class="pane secrets-pane">
    <SplitPane class="split" storage-key="command-center.secrets.split" :initial-first-pct="55" :min-first="360" :min-second="360">
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <h2>Settings</h2>
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
            <h2>{{ formTitle }}</h2>
            <button class="btn" type="button" @click="secrets.clearDraft">
              <Icon name="x" />
              <span>Clear</span>
            </button>
          </div>
          <label>
            Kind
            <select v-model="secrets.draft.kind">
              <option value="secret">Secret (encrypted, resolved at the worker)</option>
              <option value="config">Config (plain JSON, resolved by the web service)</option>
            </select>
          </label>
          <label>
            Scope
            <input v-model="secrets.draft.scope" list="secret-scopes" placeholder="github" />
          </label>
          <datalist id="secret-scopes">
            <option v-for="scope in knownScopes" :key="scope" :value="scope" />
          </datalist>
          <label>
            Name
            <input v-model="secrets.draft.name" placeholder="token" />
          </label>
          <label>
            {{ isConfig ? "Config Value (JSON)" : "Secret Value" }}
            <textarea v-model="secrets.draft.secret" :placeholder="valuePlaceholder" />
          </label>
          <label v-if="isConfig">
            Schema (JSON Schema)
            <textarea v-model="secrets.draft.schema" placeholder='{ "type": "string" } — required on first write, reused after.' />
          </label>
          <p class="hint">{{ hint }}</p>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>{{ saveLabel }}</span>
          </button>
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
    ? "Config values are plain JSON read by the web service. Reference them in WDL as config.scope.name."
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

.secret-form label {
  display: grid;
  color: #4b5663;
  font-size: 12px;
  gap: 4px;
}

.secret-form textarea {
  min-height: 130px;
}

.hint {
  color: #66717e;
  font-size: 12px;
  margin: 0;
}
</style>
