<template>
  <section class="pane res-pane">
    <div v-if="!orgs.activeOrg" class="panel empty-state">
      Select an organization on the Organization tab to manage its resources.
    </div>

    <template v-else>
      <div class="panel">
        <div class="panel-toolbar">
          <h2>Resources — {{ orgs.activeOrg.name }}</h2>
          <button class="btn" :disabled="refreshing" @click="refresh">
            <LoadingSpinner v-if="refreshing" size="sm" label="Refreshing org resources" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </div>

        <LoadingPanel
          v-if="refreshing && projectedMonthlyCents === 0 && !groups.length"
          compact
          :message="refreshMessage || 'Loading org resources…'"
        />
        <div v-else class="res-summary">
          <div class="res-stat">
            <label>Projected monthly</label>
            <div class="res-stat-value">{{ fmtCents(projectedMonthlyCents) }}</div>
          </div>
          <div class="res-stat">
            <label>Accrued (30d)</label>
            <div class="res-stat-value">{{ fmtCents(usage?.accrued_cents ?? 0) }}</div>
          </div>
          <div class="res-stat">
            <label>Monthly budget</label>
            <div class="res-stat-value">
              {{
                quota && quota.max_monthly_cents > 0
                  ? fmtCents(quota.max_monthly_cents)
                  : "unlimited"
              }}
            </div>
          </div>
        </div>

        <div v-if="budgetPct !== null" class="budget-bar">
          <div
            class="budget-fill"
            :class="{ over: budgetPct >= 100 }"
            :style="{ width: Math.min(budgetPct, 100) + '%' }"
          ></div>
        </div>
      </div>

      <div class="panel res-detail-panel">
        <div class="res-grid">
          <section class="res-card res-card-wide">
            <h3 class="res-card-title">Dedicated allocations</h3>
            <LoadingPanel
              v-if="refreshing && !groups.length"
              compact
              :message="refreshMessage || 'Loading node pools…'"
            />
            <div v-else-if="!groups.length" class="empty-state">
              No dedicated node pools. Scale one below.
            </div>
            <table v-else class="res-table">
              <thead>
                <tr>
                  <th>Backend</th>
                  <th>Kind</th>
                  <th>Desired</th>
                  <th>Rate</th>
                  <th>Monthly</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="g in groups" :key="g.backend + g.kind">
                  <td>{{ g.backend }}</td>
                  <td>{{ g.kind }}</td>
                  <td>{{ g.desired }}</td>
                  <td>{{ fmtCents(rate(g.backend, g.kind)) }}/h</td>
                  <td>{{ fmtCents(g.desired * rate(g.backend, g.kind) * HOURS_PER_MONTH) }}</td>
                </tr>
              </tbody>
            </table>
          </section>

          <section class="res-card">
            <h3 class="res-card-title">Node-hours (30d)</h3>
            <LoadingPanel
              v-if="refreshing && !usageKinds.length"
              compact
              message="Loading usage…"
            />
            <div v-else-if="!usageKinds.length" class="empty-state">No usage recorded yet.</div>
            <table v-else class="res-table">
              <thead>
                <tr>
                  <th>Kind</th>
                  <th>Node-hours</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="[kind, hours] in usageKinds" :key="kind">
                  <td>{{ kind }}</td>
                  <td>{{ hours.toFixed(2) }}</td>
                </tr>
              </tbody>
            </table>
          </section>

          <section v-if="can('org:nodes:scale')" class="res-card res-card-wide">
            <h3 class="res-card-title">Scale a pool</h3>
            <form class="res-scale" @submit.prevent="scale">
              <select v-model="scaleBackend">
                <option value="supervisor">supervisor</option>
                <option value="kubernetes">kubernetes</option>
              </select>
              <select v-model="scaleKind">
                <option value="worker">worker</option>
                <option value="waker">waker</option>
                <option value="webservice">webservice</option>
              </select>
              <input v-model.number="scaleDesired" type="number" min="0" />
              <button class="btn btn-primary" type="submit" :disabled="scaling">
                <LoadingSpinner v-if="scaling" size="sm" label="Scaling org nodes" />
                {{ scaling ? "Scaling…" : "Set desired" }}
              </button>
              <span class="res-preview">
                ≈ {{ fmtCents(scaleDesired * rate(scaleBackend, scaleKind) * HOURS_PER_MONTH) }}/mo
              </span>
            </form>
            <p class="res-note">
              A worker pool with a positive count makes this org's workflows route to its dedicated,
              <span class="mono">org={{ orgs.activeOrg.slug }}</span
              >-labeled workers.
            </p>
          </section>
        </div>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import {
  orgResourcesService,
  type OrgQuota,
  type OrgResourceGroup,
  type OrgUsage,
  type RateCard,
} from "../../core/services";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useCan } from "../composables/useCan";
import { useOperationLoading } from "../composables/useOperationLoading";

const HOURS_PER_MONTH = 730;

const orgs = useOrgsStore();
const { can } = useCan();
const refreshing = ref(false);
const { isLoading: loadingNodes, loadingMessage: refreshMessage } =
  useOperationLoading("Loading org nodes");
const { isLoading: scalingNodes } = useOperationLoading("Scaling org nodes");
const scaling = computed(() => scalingNodes.value);
const groups = ref<OrgResourceGroup[]>([]);
const projectedMonthlyCents = ref(0);
const quota = ref<OrgQuota | null>(null);
const usage = ref<OrgUsage | null>(null);
const rateCard = ref<RateCard>({ entries: [] });

const scaleBackend = ref("supervisor");
const scaleKind = ref("worker");
const scaleDesired = ref(1);

const usageKinds = computed(() => Object.entries(usage.value?.node_hours ?? {}));
const budgetPct = computed(() => {
  if (!quota.value || quota.value.max_monthly_cents <= 0) {
    return null;
  }

  return Math.round((projectedMonthlyCents.value / quota.value.max_monthly_cents) * 100);
});

function rate(backend: string, kind: string): number {
  return (
    rateCard.value.entries.find((e) => e.backend === backend && e.kind === kind)?.hourly_cents ?? 0
  );
}

function fmtCents(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

async function refresh() {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    groups.value = [];
    projectedMonthlyCents.value = 0;
    quota.value = null;
    usage.value = null;
    return;
  }

  refreshing.value = true;

  try {
    rateCard.value = await orgResourcesService.fetchRateCard().catch(() => ({ entries: [] }));
    const nodes = await orgResourcesService.fetchNodes(orgId).catch(() => ({
      groups: [],
      projected_monthly_cents: 0,
    }));
    groups.value = nodes.groups;
    projectedMonthlyCents.value = nodes.projected_monthly_cents;
    quota.value = await orgResourcesService.fetchQuota(orgId).catch(() => null);
    usage.value = await orgResourcesService.fetchUsage(orgId).catch(() => null);
  } finally {
    refreshing.value = false;
  }
}

async function scale() {
  const orgId = orgs.activeOrgId;

  if (!orgId) {
    return;
  }

  try {
    await orgResourcesService.scaleNodes(orgId, {
      backend: scaleBackend.value,
      kind: scaleKind.value,
      desired: Math.max(0, Math.floor(scaleDesired.value)),
    });
    await refresh();
  } catch {
    // runOperation surfaces errors via toast.
  }
}

watch(() => orgs.activeOrgId, refresh);
onMounted(refresh);
</script>

<style scoped>
.res-pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.res-summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.res-stat label {
  display: block;
  margin-bottom: 4px;
  color: var(--text-muted);
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.res-stat-value {
  font-size: 22px;
  font-weight: 600;
}

.budget-bar {
  margin-top: 12px;
  height: 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  overflow: hidden;
}

.budget-fill {
  height: 100%;
  background: var(--accent);
}

.budget-fill.over {
  background: var(--danger-fg);
}

.res-grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.res-card {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 12px 14px;
}

.res-card-wide {
  grid-column: 1 / -1;
}

.res-card-title {
  margin: 0 0 10px;
  font-size: 14px;
}

.res-table {
  width: 100%;
  border-collapse: collapse;
}

.res-table th,
.res-table td {
  text-align: left;
  padding: 8px 6px;
  border-bottom: 1px solid var(--border);
}

.res-scale {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}

.res-scale input {
  width: 90px;
}

.res-preview {
  color: var(--text-muted);
  font-size: 13px;
}

.res-note {
  color: var(--text-muted);
  font-size: 13px;
  margin: 10px 0 0;
}

.mono {
  font-family: var(--font-mono);
}

@media (max-width: 820px) {
  .res-summary {
    grid-template-columns: 1fr;
  }

  .res-grid {
    grid-template-columns: 1fr;
  }
}
</style>
