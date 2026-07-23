<template>
  <Modal title="Share workflow" width="min(560px, calc(100vw - 32px))" @close="emit('close')">
    <section class="form-section">
      <h3>Access</h3>
      <table v-if="grants.length" class="w-full border-collapse text-xs">
        <thead>
          <tr>
            <th class="border-b border-border-subtle px-2 py-1.5 text-left">Principal</th>
            <th class="border-b border-border-subtle px-2 py-1.5 text-left">Type</th>
            <th class="border-b border-border-subtle px-2 py-1.5 text-left">Permission</th>
            <th class="border-b border-border-subtle px-2 py-1.5 text-left"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="grant in grants" :key="String(grant.id)">
            <td class="mono border-b border-border-subtle px-2 py-1.5 text-left">{{ grant.principal_id }}</td>
            <td class="border-b border-border-subtle px-2 py-1.5 text-left">{{ grant.principal_type }}</td>
            <td class="border-b border-border-subtle px-2 py-1.5 text-left">{{ grant.permission }}</td>
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
      <form class="flex flex-wrap items-end gap-2" @submit.prevent="add">
        <label class="grid gap-1 text-xs text-fg-subtle">
          Principal
          <select v-model="principalType">
            <option value="user">user</option>
            <option value="team">team</option>
          </select>
        </label>
        <label class="grid gap-1 text-xs text-fg-subtle">
          Principal ID (UUID)
          <input v-model="principalId" placeholder="user or team id" />
        </label>
        <label class="grid gap-1 text-xs text-fg-subtle">
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
import { workflowSharingService } from "../../../core/services";
import Modal from "../shared/Modal.vue";
import Button from "../shared/Button.vue";
import type { JsonRecord } from "../../../core/domain/models";

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
    grants.value = await workflowSharingService.listGrants(props.workflowId);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

async function add() {
  busy.value = true;
  error.value = "";

  try {
    await workflowSharingService.createGrant(
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
    await workflowSharingService.revokeGrant(props.workflowId, grantId);
    await refresh();
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

onMounted(refresh);
</script>
