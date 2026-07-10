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
          <tr
            v-for="group in groups"
            :key="`${group.backend}-${group.kind}`"
            :class="{ 'node-pools-ghost': !group.manageable }"
          >
            <td>{{ group.backend }}</td>
            <td>{{ group.kind }}</td>
            <td>{{ group.desired }}</td>
            <td>{{ group.available }}</td>
            <td class="node-pools-actions">
              <span v-if="!group.manageable" class="node-pools-unconfigured">
                Not configured on this backend
              </span>
              <template v-else>
                <button
                  class="btn"
                  :disabled="busy"
                  title="Spin up one node"
                  @click="scaleBy(group, 1)"
                >
                  <Icon name="play" />
                  <span>Spin up</span>
                </button>
                <button
                  class="btn"
                  :disabled="busy || group.desired <= minDesired(group)"
                  title="Scale down one node"
                  @click="scaleBy(group, -1)"
                >
                  <Icon name="stop" />
                  <span>-1</span>
                </button>
                <button
                  class="btn"
                  :disabled="busy || isProtected(group) || group.desired === 0"
                  :title="
                    isProtected(group)
                      ? `${group.kind} is a control-plane node and cannot be scaled to zero from here`
                      : 'Scale to zero'
                  "
                  @click="scaleTo(group, 0)"
                >
                  <span>Stop all</span>
                </button>
              </template>
            </td>
          </tr>
          <tr v-if="!groups.length">
            <td colspan="5" class="empty-state">No node groups reported.</td>
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
  nodePoolsService,
  type NodeBackendInfo,
  type ProvisionedGroup,
} from "../../../core/services";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useAuthStore } from "../../../ui/adapters/pinia/auth";
import { errorMessage } from "../../../core/utils/format";

const app = useAppStore();
const auth = useAuthStore();

const isAdmin = computed(() => !auth.required || auth.user?.is_admin === true);

// the scale-to-zero floor is backend-provided: control-plane kinds (webservice/postgres) report a
// min_desired of one so they cannot be scaled to zero from here, and any future protected kind is
// honored without a ui change.
function minDesired(group: ProvisionedGroup): number {
  return group.min_desired ?? 0;
}

function isProtected(group: ProvisionedGroup): boolean {
  return minDesired(group) > 0;
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
    backends.value = (await nodePoolsService.fetchBackends()).backends;
    groups.value = await nodePoolsService.fetchNodes();
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
    await nodePoolsService.scale({ backend: group.backend, kind: group.kind, desired });
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
/* ghost rows are kinds this backend has no template/deployment for; shown for awareness. */
.node-pools-ghost td {
  opacity: 0.5;
}
.node-pools-unconfigured {
  font-size: 0.8rem;
  font-style: italic;
  opacity: 0.85;
}
</style>
