<template>
  <div v-if="isAdmin" class="panel node-pools-panel">
    <div class="panel-toolbar">
      <h2>Node Pools</h2>
      <button class="btn" :disabled="loading" @click="refresh">
        <Icon name="refresh" />
        <span>Refresh</span>
      </button>
    </div>

    <p class="node-pools-hint">
      Spin up or scale down runtime nodes on demand across the configured provisioning backends.
      Desired/Ready are what the orchestrator reports for the workload; they can differ from the
      Replicas list above, which only counts nodes that have registered and are heartbeating.
    </p>

    <div v-if="!backends.length" class="empty-state">
      No provisioning backends are configured on the web service.
    </div>

    <div v-else class="node-pools-table-wrap">
      <table class="node-pools-table">
        <thead>
          <tr>
            <th>Backend</th>
            <th>Kind</th>
            <th>Desired</th>
            <th>Ready</th>
            <th class="node-pools-actions-col">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="group in groups" :key="`${group.backend}-${group.kind}`">
            <td>{{ group.backend }}</td>
            <td>{{ group.kind }}</td>
            <td>{{ group.desired }}</td>
            <td>{{ group.available }}</td>
            <td class="node-pools-actions">
              <button
                class="btn"
                :disabled="busy || !group.manageable"
                title="Spin up one node"
                @click="scaleBy(group, 1)"
              >
                <Icon name="play" />
                <span>Spin up</span>
              </button>
              <button
                class="btn"
                :disabled="busy || !group.manageable || group.desired <= minDesired(group)"
                title="Scale down one node"
                @click="scaleBy(group, -1)"
              >
                <Icon name="stop" />
                <span>-1</span>
              </button>
              <button
                class="btn"
                :disabled="busy || !group.manageable || isProtected(group) || group.desired === 0"
                :title="
                  isProtected(group)
                    ? `${group.kind} is a control-plane node and cannot be scaled to zero from here`
                    : 'Scale to zero'
                "
                @click="scaleTo(group, 0)"
              >
                <span>Stop all</span>
              </button>
            </td>
          </tr>
          <tr v-if="!groups.length">
            <td colspan="5" class="empty-state">No manageable node groups reported.</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="error" class="empty-state">{{ error }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Icon from "./Icon.vue";
import {
  fetchNodeBackends,
  fetchNodes,
  scaleNodes,
  type NodeBackendInfo,
  type ProvisionedGroup,
} from "../../../api/commandCenterApi";
import { useAppStore } from "../../../stores/app";
import { useAuthStore } from "../../../stores/auth";
import { errorMessage } from "../../../utils/format";

const app = useAppStore();
const auth = useAuthStore();

const isAdmin = computed(() => !auth.required || auth.user?.is_admin === true);

// webservice and postgres back the control plane; scaling them to zero from here would take down
// the api or database, so keep a floor of one replica on these kinds.
const PROTECTED_KINDS = new Set(["webservice", "postgres"]);

function isProtected(group: ProvisionedGroup): boolean {
  return PROTECTED_KINDS.has(group.kind);
}

function minDesired(group: ProvisionedGroup): number {
  return isProtected(group) ? 1 : 0;
}

const backends = ref<NodeBackendInfo[]>([]);
const groups = ref<ProvisionedGroup[]>([]);
const loading = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);

async function refresh() {
  if (!isAdmin.value) {
    return;
  }

  loading.value = true;
  error.value = null;

  try {
    backends.value = (await fetchNodeBackends()).backends;
    groups.value = await fetchNodes();
  } catch (err) {
    error.value = errorMessage(err) || "Failed to load node pools";
  } finally {
    loading.value = false;
  }
}

async function scaleTo(group: ProvisionedGroup, desired: number) {
  busy.value = true;
  error.value = null;

  try {
    await app.runOperation(`Scaling ${group.kind} to ${String(desired)}`, () =>
      scaleNodes({ backend: group.backend, kind: group.kind, desired }),
    );
    await refresh();
  } catch (err) {
    error.value = errorMessage(err) || "Scale request failed";
  } finally {
    busy.value = false;
  }
}

function scaleBy(group: ProvisionedGroup, delta: number) {
  return scaleTo(group, Math.max(minDesired(group), group.desired + delta));
}

onMounted(refresh);
</script>

<style scoped>
.node-pools-panel {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  flex: 0 0 auto;
  height: min(360px, 42vh);
  min-height: 140px;
  padding: 1rem;
  /* let operators drag the pane taller to see more node groups at once. */
  resize: vertical;
  overflow: auto;
}
.node-pools-hint {
  margin: 0;
  flex: 0 0 auto;
  opacity: 0.75;
  font-size: 0.85rem;
}
.node-pools-table-wrap {
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border-faint);
  border-radius: var(--radius);
}
.node-pools-table {
  width: max(100%, 640px);
  border-collapse: collapse;
  table-layout: auto;
  font-size: 0.85rem;
}
.node-pools-table th,
.node-pools-table td {
  text-align: left;
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid var(--border, rgba(255, 255, 255, 0.08));
  overflow: visible;
  text-overflow: clip;
}
.node-pools-actions-col {
  width: 1%;
  white-space: nowrap;
}
.node-pools-actions {
  display: flex;
  gap: 0.4rem;
  white-space: nowrap;
}
</style>
