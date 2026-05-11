<template>
  <section class="pane secrets-pane">
    <SplitPane class="split" storage-key="command-center.secrets.split" :initial-first-pct="55" :min-first="360" :min-second="360">
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <h2>Secrets</h2>
            <div>
              <button @click="secrets.refreshSecrets">Refresh</button>
              <button :disabled="!secrets.selectedSecret" @click="secrets.deleteSelectedSecret">Remove</button>
            </div>
          </div>
          <DataTable>
            <table>
              <thead>
                <tr>
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
                  <td>{{ secret.scope }}</td>
                  <td>{{ secret.name }}</td>
                  <td><code>{{ secretRef(secret.scope, secret.name) }}</code></td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
      <template #second>
        <form class="panel secret-form" @submit.prevent="secrets.saveDraft">
          <div class="panel-toolbar">
            <h2>{{ secrets.selectedSecret ? "Update Secret" : "Add Secret" }}</h2>
            <button type="button" @click="secrets.clearDraft">Clear</button>
          </div>
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
            Secret Value
            <textarea v-model="secrets.draft.secret" placeholder="Paste the secret value to add or replace it." />
          </label>
          <p class="hint">Secret values are not shown after saving. Select an existing secret, enter a new value, and save to replace it.</p>
          <button type="submit">Save Secret</button>
        </form>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useProvidersStore } from "../stores/providers";
import { useSecretsStore } from "../stores/secrets";
import { secretKey, secretRef } from "../utils/secrets";

const secrets = useSecretsStore();
const providers = useProvidersStore();

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
