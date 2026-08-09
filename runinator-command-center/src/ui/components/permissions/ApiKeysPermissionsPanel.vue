<template>
  <div class="min-h-0 overflow-hidden">
    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div><h3 class="m-0 text-sm font-semibold text-fg">API Keys</h3><p class="m-0 text-xs text-fg-muted">{{ scopeLabel }}</p></div>
        <div class="btn-row">
          <button class="btn btn-primary" type="button" @click="openNew"><Icon name="plus" /><span>New Key</span></button>
          <button class="btn" type="button" @click="permissions.refreshApiKeys"><Icon name="refresh" /><span>Refresh</span></button>
        </div>
      </div>
      <LoadingPanel v-if="loading && !permissions.visibleApiKeys.length" compact :message="loadingMessage || 'Loading API keys…'" />
      <DataTable v-else>
        <table>
          <thead><tr><th>Name</th><th>Owner</th><th>Prefix</th><th>Status</th><th>Last Used</th></tr></thead>
          <tbody>
            <tr v-for="apiKey in permissions.visibleApiKeys" :key="String(apiKey.id)" class="cursor-pointer" :class="{ selected: permissions.selectedApiKeyId === apiKey.id, muted: apiKey.disabled }" @click="openEdit(apiKey)">
              <td>{{ apiKey.name }}</td><td>{{ ownerLabel(apiKey) }}</td><td><code>{{ apiKey.key_prefix }}</code></td>
              <td>{{ apiKey.disabled ? "revoked" : "active" }}</td><td>{{ apiKey.last_used_at ? formatDate(apiKey.last_used_at) : "-" }}</td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </section>

    <div v-if="modalOpen" class="modal-backdrop" @click.self="closeModal">
      <form class="modal w-full max-w-[860px]" @submit.prevent="save">
        <header class="modal-header"><h2>{{ permissions.selectedApiKey ? "Edit API Key" : "Create API Key" }}</h2><button class="btn btn-ghost" type="button" @click="closeModal"><Icon name="x" /></button></header>
        <div v-if="permissions.revealedApiKey" class="flex items-start justify-between gap-3 rounded-md border border-accent bg-accent-soft px-3 py-2.5">
          <div class="grid min-w-0 gap-1.5"><span class="text-xs text-fg-muted">Secret for {{ permissions.revealedApiKey.api_key.name }}</span><code class="break-words">{{ permissions.revealedApiKey.secret }}</code></div>
          <div class="btn-row">
            <button class="btn btn-sm" type="button" @click="copySecret"><Icon name="key" /><span>Copy</span></button>
            <button class="btn btn-sm" type="button" @click="permissions.clearRevealedApiKey"><Icon name="x" /><span>Clear</span></button>
          </div>
        </div>
        <div class="form-grid !grid-cols-1">
          <label><span>Name</span><input v-model="permissions.apiKeyDraft.name" autocomplete="off" /></label>
          <label><span>Owner</span><select v-model="owner" :disabled="Boolean(permissions.selectedApiKey)"><option value="service">Service key</option><option v-for="user in ownerOptions" :key="String(user.id)" :value="String(user.id)">{{ user.username }}</option></select></label>
          <label><span>Expires At</span><input v-model="permissions.apiKeyDraft.expires_at" type="datetime-local" /></label>
          <label class="inline-flex items-center gap-1.5 text-[13px] text-fg"><input v-model="permissions.apiKeyDraft.disabled" type="checkbox" /><span>Disabled</span></label>
        </div>
        <div class="modal-actions">
          <button class="btn btn-danger" type="button" :disabled="!permissions.selectedApiKey" @click="confirmRevoke"><Icon name="trash" /><span>Revoke</span></button>
          <button class="btn" type="button" :disabled="!permissions.selectedApiKey || permissions.selectedApiKey.disabled" @click="confirmRotate"><Icon name="refresh" /><span>Rotate</span></button>
          <button class="btn" type="button" @click="closeModal">Cancel</button>
          <button class="btn btn-primary" type="submit"><Icon name="save" /><span>{{ permissions.selectedApiKey ? "Save Key" : "Create Key" }}</span></button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { ApiKey } from "../../../core/domain/models";
import { formatDate } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import { usePermissionsStore } from "../../adapters/pinia/permissions";
import { useOperationLoading } from "../../composables/useOperationLoading";
import DataTable from "../shared/DataTable.vue";
import Icon from "../shared/Icon.vue";
import LoadingPanel from "../shared/LoadingPanel.vue";

const app = useAppStore();
const permissions = usePermissionsStore();
const { isLoading: loading, loadingMessage } = useOperationLoading(["Loading API keys"]);
const modalOpen = ref(false);
const owner = computed({
  get() {
    if (permissions.apiKeyDraft.is_service) {
      return "service";
    }

    return permissions.apiKeyDraft.user_id
      ? permissions.apiKeyDraft.user_id
      : (permissions.selectedUserId ?? "");
  },
  set(value: string) {
    permissions.apiKeyDraft.is_service = value === "service";
    permissions.apiKeyDraft.user_id = value === "service" ? "" : value;
  },
});
const ownerOptions = computed(() => permissions.selectedUser ? [permissions.selectedUser] : permissions.users);
const scopeLabel = computed(() => permissions.selectedUser ? `Showing service keys and keys owned by ${permissions.selectedUser.username}.` : "Showing all API keys.");

function openNew() {
  permissions.clearApiKeyDraft();
  modalOpen.value = true;
}

function openEdit(apiKey: ApiKey) {
  permissions.selectApiKey(apiKey);
  modalOpen.value = true;
}

function closeModal() {
  modalOpen.value = false;
}

async function save() {
  await permissions.saveApiKeyDraft();
  if (!app.errorText && !permissions.revealedApiKey) {closeModal();}
}

function confirmRevoke() {
  const key = permissions.selectedApiKey;
  if (!key || !window.confirm(`Revoke API key ${key.name}?`)) {return;}
  void permissions.revokeSelectedApiKey().then(closeModal);
}

function confirmRotate() {
  const key = permissions.selectedApiKey;
  if (!key || !window.confirm(`Rotate API key ${key.name}? The old secret will stop working.`)) {return;}
  void permissions.rotateSelectedApiKey();
}

async function copySecret() {
  const secret = permissions.revealedApiKey?.secret;
  if (!secret) {return;}

  try {
    await navigator.clipboard.writeText(secret);
    app.setStatus("API key secret copied.");
  } catch {
    app.setError("Unable to copy API key secret.");
  }
}

function ownerLabel(apiKey: ApiKey): string {
  if (apiKey.is_service) {return "service";}
  return permissions.users.find((user) => user.id === apiKey.user_id)?.username ?? apiKey.user_id ?? "user";
}
</script>
