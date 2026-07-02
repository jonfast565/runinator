<template>
  <Modal title="Share workflow" width="min(560px, calc(100vw - 32px))" @close="emit('close')">
    <section class="form-section">
      <h3>Access</h3>
      <table v-if="grants.length" class="grants-table">
        <thead>
          <tr>
            <th>Principal</th>
            <th>Type</th>
            <th>Permission</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="grant in grants" :key="String(grant.id)">
            <td class="mono">{{ grant.principal_id }}</td>
            <td>{{ grant.principal_type }}</td>
            <td>{{ grant.permission }}</td>
            <td>
              <Button size="sm" variant="ghost" @click="revoke(String(grant.id))">Remove</Button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="hint">
        No grants yet. The creator owns this workflow; add grants to share it.
      </p>
    </section>

    <section class="form-section">
      <h3>Add grant</h3>
      <form class="add-grant" @submit.prevent="add">
        <label>
          Principal
          <select v-model="principalType">
            <option value="user">user</option>
            <option value="team">team</option>
          </select>
        </label>
        <label>
          Principal ID (UUID)
          <input v-model="principalId" placeholder="user or team id" />
        </label>
        <label>
          Permission
          <select v-model="permission">
            <option value="view">view</option>
            <option value="run">run</option>
            <option value="edit">edit</option>
            <option value="own">own</option>
          </select>
        </label>
        <Button variant="primary" type="submit" :loading="busy" :disabled="!principalId"
          >Add</Button
        >
      </form>
      <p v-if="error" class="error">{{ error }}</p>
    </section>
  </Modal>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  createWorkflowGrant,
  listWorkflowGrants,
  revokeWorkflowGrant,
} from "../../../api/commandCenterApi";
import Modal from "../shared/Modal.vue";
import Button from "../shared/Button.vue";
import type { JsonRecord } from "../../../types/models";

const props = defineProps<{ workflowId: string }>();
const emit = defineEmits<{ close: [] }>();

const grants = ref<JsonRecord[]>([]);
const principalType = ref<"user" | "team">("user");
const principalId = ref("");
const permission = ref<"view" | "run" | "edit" | "own">("view");
const error = ref("");
const busy = ref(false);

async function refresh() {
  error.value = "";

  try {
    grants.value = await listWorkflowGrants(props.workflowId);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

async function add() {
  busy.value = true;
  error.value = "";

  try {
    await createWorkflowGrant(
      props.workflowId,
      principalType.value,
      principalId.value.trim(),
      permission.value,
    );
    principalId.value = "";
    await refresh();
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    busy.value = false;
  }
}

async function revoke(grantId: string) {
  try {
    await revokeWorkflowGrant(props.workflowId, grantId);
    await refresh();
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

onMounted(refresh);
</script>

<style scoped>
.form-section {
  margin-top: 14px;
}

.grants-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.grants-table th,
.grants-table td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
}

.add-grant {
  display: flex;
  flex-wrap: wrap;
  align-items: end;
  gap: 8px;
}

.add-grant label {
  display: grid;
  gap: 4px;
  font-size: 12px;
  color: var(--text-subtle);
}

.add-grant input,
.add-grant select {
  padding: 6px 8px;
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  font: inherit;
}

.hint {
  color: var(--text-muted);
  font-size: 12px;
}

.error {
  color: var(--danger-fg);
  font-size: 12px;
}
</style>
