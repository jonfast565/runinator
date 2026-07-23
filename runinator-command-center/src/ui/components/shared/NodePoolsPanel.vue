<template>
  <div v-if="canScale" class="panel node-pools-panel">
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
import { useCan } from "../../../ui/composables/useCan";
import { errorMessage } from "../../../core/utils/format";

const app = useAppStore();
const { can } = useCan();

// global worker-node scaling is a platform capability (backend: nodes:scale).
const canScale = computed(() => can("nodes:scale"));

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
  if (!canScale.value) {
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

