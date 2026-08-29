<template>
  <section class="pane h-full overflow-hidden">
    <div class="flex h-full min-h-0 flex-col gap-2.5 overflow-auto">
      <div class="panel shrink-0">
        <div class="panel-toolbar">
          <h2 class="m-0 text-base font-semibold text-fg">
            Resources &amp; Billing<template v-if="orgs.activeOrg">
              — {{ orgs.activeOrg.name }}</template
            >
          </h2>
          <button class="btn" :disabled="refreshing || !orgs.activeOrg" @click="refresh">
            <LoadingSpinner v-if="refreshing" size="sm" label="Refreshing org resources" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </div>

        <EmptyState
          v-if="!orgs.activeOrg"
          icon="shield"
          title="No organization selected"
          :loading="refreshing"
          loading-message="Loading organizations…"
        />
        <LoadingPanel
          v-else-if="refreshing && !projectedMonthlyCents && !groups.length"
          compact
          :message="refreshMessage || 'Loading org resources…'"
        />
        <template v-else>
          <div class="metrics-row">
            <MetricCard label="Projected monthly" :value="fmtCents(projectedMonthlyCents)" />
            <MetricCard label="Accrued (30d)" :value="fmtCents(usage?.accrued_cents ?? 0)" />
            <MetricCard
              label="Monthly budget"
              :value="
                quota && quota.max_monthly_cents > 0
                  ? fmtCents(quota.max_monthly_cents)
                  : 'Unlimited'
              "
            />
          </div>

          <div v-if="budgetPct !== null" class="grid gap-1.5">
            <div class="flex items-baseline justify-between gap-2 text-xs text-fg-muted">
              <span>Projected spend against budget</span>
              <span :class="budgetPct >= 100 ? 'font-semibold text-danger-fg' : ''"
                >{{ budgetPct }}%</span
              >
            </div>
            <div
              class="h-2 overflow-hidden rounded-pill bg-surface-sunken"
              role="progressbar"
              :aria-valuenow="budgetPct"
              :aria-valuemin="0"
              :aria-valuemax="100"
            >
              <div
                class="h-full transition-[width] duration-300 ease-out"
                :class="budgetPct >= 100 ? 'bg-danger' : 'bg-accent'"
                :style="{ width: Math.min(budgetPct, 100) + '%' }"
              />
            </div>
          </div>
        </template>
      </div>

      <template v-if="orgs.activeOrg">
        <div class="panel shrink-0">
          <div class="panel-toolbar">
            <h3 class="m-0 text-sm font-semibold text-fg">Dedicated allocations</h3>
            <span class="rounded-pill bg-surface-subtle px-2 py-0.5 text-xs text-fg-subtle"
              >{{ groups.length }} pool(s)</span
            >
          </div>
          <LoadingPanel
            v-if="refreshing && !groups.length"
            compact
            :message="refreshMessage || 'Loading node pools…'"
          />
          <EmptyState
            v-else-if="!groups.length"
            compact
            icon="box"
            title="No dedicated node pools"
          />
          <DataTable v-else>
            <table>
              <thead>
                <tr>
                  <th>Backend</th>
                  <th>Kind</th>
                  <th class="text-right">Desired</th>
                  <th class="text-right">Rate</th>
                  <th class="text-right">Monthly</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="g in groups" :key="g.backend + g.kind">
                  <td>{{ g.backend }}</td>
                  <td>{{ g.kind }}</td>
                  <td class="text-right tabular-nums">{{ g.desired }}</td>
                  <td class="text-right tabular-nums">{{ fmtCents(rate(g.backend, g.kind)) }}/h</td>
                  <td class="text-right tabular-nums">
                    {{ fmtCents(g.desired * rate(g.backend, g.kind) * HOURS_PER_MONTH) }}
                  </td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>

        <div class="panel shrink-0">
          <div class="panel-toolbar">
            <h3 class="m-0 text-sm font-semibold text-fg">Node-hours (30d)</h3>
          </div>
          <LoadingPanel v-if="refreshing && !usageKinds.length" compact message="Loading usage…" />
          <EmptyState
            v-else-if="!usageKinds.length"
            compact
            icon="clock"
            title="No usage recorded yet"
          />
          <DataTable v-else>
            <table>
              <thead>
                <tr>
                  <th>Kind</th>
                  <th class="text-right">Node-hours</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="[kind, hours] in usageKinds" :key="kind">
                  <td>{{ kind }}</td>
                  <td class="text-right tabular-nums">{{ hours.toFixed(2) }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>

        <div v-if="can('org:nodes:scale')" class="panel shrink-0">
          <div class="panel-toolbar">
            <h3 class="m-0 text-sm font-semibold text-fg">Scale a pool</h3>
          </div>
          <form class="flex flex-wrap items-end gap-2" @submit.prevent="scale">
            <label class="grid gap-1 text-xs text-fg-muted">
              <span>Backend</span>
              <select v-model="scaleBackend" class="w-auto min-w-36">
                <option value="supervisor">supervisor</option>
                <option value="kubernetes">kubernetes</option>
              </select>
            </label>
            <label class="grid gap-1 text-xs text-fg-muted">
              <span>Kind</span>
              <select v-model="scaleKind" class="w-auto min-w-36">
                <option value="worker">worker</option>
                <option value="waker">waker</option>
                <option value="webservice">webservice</option>
              </select>
            </label>
            <label class="grid gap-1 text-xs text-fg-muted">
              <span>Desired nodes</span>
              <input
                v-model.number="scaleDesired"
                class="w-[90px]"
                type="number"
                min="0"
                step="1"
                required
              />
            </label>
            <button class="btn btn-primary" type="submit" :disabled="scaling">
              <LoadingSpinner v-if="scaling" size="sm" label="Scaling org nodes" />
              <span>{{ scaling ? "Scaling…" : "Set desired" }}</span>
            </button>
            <span class="pb-1.5 text-[13px] text-fg-muted">
              ≈ {{ fmtCents(scaleDesired * rate(scaleBackend, scaleKind) * HOURS_PER_MONTH) }}/mo
            </span>
          </form>
        </div>
      </template>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
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
const { loadingMessage: refreshMessage } = useOperationLoading("Loading org nodes");
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
