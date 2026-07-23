<template>
  <section class="pane flex flex-col gap-2.5 overflow-auto">
    <div v-if="!orgs.activeOrg" class="panel py-3.5 text-fg-muted">
      Select an organization on the Organization tab to manage its resources.
    </div>

    <template v-else>
      <div class="panel">
        <div class="panel-toolbar">
          <h2 class="m-0 text-base font-semibold text-fg">
            Resources — {{ orgs.activeOrg.name }}
          </h2>
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
        <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <div>
            <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
              >Projected monthly</label
            >
            <div class="text-[22px] font-semibold">{{ fmtCents(projectedMonthlyCents) }}</div>
          </div>
          <div>
            <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
              >Accrued (30d)</label
            >
            <div class="text-[22px] font-semibold">{{ fmtCents(usage?.accrued_cents ?? 0) }}</div>
          </div>
          <div>
            <label class="mb-1 block text-xs tracking-wide text-fg-muted uppercase"
              >Monthly budget</label
            >
            <div class="text-[22px] font-semibold">
              {{
                quota && quota.max_monthly_cents > 0
                  ? fmtCents(quota.max_monthly_cents)
                  : "unlimited"
              }}
            </div>
          </div>
        </div>

        <div
          v-if="budgetPct !== null"
          class="mt-3 h-2 overflow-hidden rounded-pill bg-surface-subtle"
        >
          <div
            class="h-full bg-accent"
            :class="{ 'bg-danger-fg': budgetPct >= 100 }"
            :style="{ width: Math.min(budgetPct, 100) + '%' }"
          ></div>
        </div>
      </div>

      <div class="panel">
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
          <section
            class="col-span-full rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3"
          >
            <h3 class="m-0 mb-2.5 text-sm font-semibold text-fg">Dedicated allocations</h3>
            <LoadingPanel
              v-if="refreshing && !groups.length"
              compact
              :message="refreshMessage || 'Loading node pools…'"
            />
            <div v-else-if="!groups.length" class="py-3.5 text-fg-muted">
              No dedicated node pools. Scale one below.
            </div>
            <table v-else class="w-full border-collapse">
              <thead>
                <tr>
                  <th class="border-b border-border px-1.5 py-2 text-left">Backend</th>
                  <th class="border-b border-border px-1.5 py-2 text-left">Kind</th>
                  <th class="border-b border-border px-1.5 py-2 text-left">Desired</th>
                  <th class="border-b border-border px-1.5 py-2 text-left">Rate</th>
                  <th class="border-b border-border px-1.5 py-2 text-left">Monthly</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="g in groups" :key="g.backend + g.kind">
                  <td class="border-b border-border px-1.5 py-2 text-left">{{ g.backend }}</td>
                  <td class="border-b border-border px-1.5 py-2 text-left">{{ g.kind }}</td>
                  <td class="border-b border-border px-1.5 py-2 text-left">{{ g.desired }}</td>
                  <td class="border-b border-border px-1.5 py-2 text-left">
                    {{ fmtCents(rate(g.backend, g.kind)) }}/h
                  </td>
                  <td class="border-b border-border px-1.5 py-2 text-left">
                    {{ fmtCents(g.desired * rate(g.backend, g.kind) * HOURS_PER_MONTH) }}
                  </td>
                </tr>
              </tbody>
            </table>
          </section>

          <section class="rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3">
            <h3 class="m-0 mb-2.5 text-sm font-semibold text-fg">Node-hours (30d)</h3>
            <LoadingPanel
              v-if="refreshing && !usageKinds.length"
              compact
              message="Loading usage…"
            />
            <div v-else-if="!usageKinds.length" class="py-3.5 text-fg-muted">
              No usage recorded yet.
            </div>
            <table v-else class="w-full border-collapse">
              <thead>
                <tr>
                  <th class="border-b border-border px-1.5 py-2 text-left">Kind</th>
                  <th class="border-b border-border px-1.5 py-2 text-left">Node-hours</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="[kind, hours] in usageKinds" :key="kind">
                  <td class="border-b border-border px-1.5 py-2 text-left">{{ kind }}</td>
                  <td class="border-b border-border px-1.5 py-2 text-left">
                    {{ hours.toFixed(2) }}
                  </td>
                </tr>
              </tbody>
            </table>
          </section>

          <section
            v-if="can('org:nodes:scale')"
            class="col-span-full rounded-md border border-border-subtle bg-surface-subtle px-3.5 py-3"
          >
            <h3 class="m-0 mb-2.5 text-sm font-semibold text-fg">Scale a pool</h3>
            <form class="flex flex-wrap items-center gap-2" @submit.prevent="scale">
              <select v-model="scaleBackend">
                <option value="supervisor">supervisor</option>
                <option value="kubernetes">kubernetes</option>
              </select>
              <select v-model="scaleKind">
                <option value="worker">worker</option>
                <option value="waker">waker</option>
                <option value="webservice">webservice</option>
              </select>
              <input v-model.number="scaleDesired" class="w-[90px]" type="number" min="0" />
              <button class="btn btn-primary" type="submit" :disabled="scaling">
                <LoadingSpinner v-if="scaling" size="sm" label="Scaling org nodes" />
                {{ scaling ? "Scaling…" : "Set desired" }}
              </button>
              <span class="text-[13px] text-fg-muted">
                ≈ {{ fmtCents(scaleDesired * rate(scaleBackend, scaleKind) * HOURS_PER_MONTH) }}/mo
              </span>
            </form>
            <p class="mt-2.5 mb-0 text-[13px] text-fg-muted">
              A worker pool with a positive count makes this org's workflows route to its dedicated,
              <span class="font-mono">org={{ orgs.activeOrg.slug }}</span
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

