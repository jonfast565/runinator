<template>
  <section class="pane h-full overflow-hidden">
    <div class="flex h-full min-h-0 flex-col gap-2.5 overflow-auto">
      <div class="panel shrink-0">
        <PanelHeader
          icon="box"
          eyebrow="Capacity &amp; cost"
          description="Review organization limits, projected cost, recent usage, and dedicated node allocations."
        >
          <template #title>
            Resources &amp; Billing<template v-if="orgs.activeOrg">
              — {{ orgs.activeOrg.name }}</template
            >
          </template>
          <button class="btn" :disabled="refreshing || !orgs.activeOrg" @click="refresh">
            <LoadingSpinner v-if="refreshing" size="sm" label="Refreshing org resources" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </PanelHeader>

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
            <div class="flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">Dedicated allocations</h3>
              <HelpBubble
                text="Dedicated node pools reserved for the active organization and their projected monthly cost."
                label="About dedicated allocations"
              />
            </div>
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
          </DataTable>
        </div>

        <div class="panel shrink-0">
          <div class="panel-toolbar">
            <div class="flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">Node-hours (30d)</h3>
              <HelpBubble
                text="Runtime usage accrued by node kind during the last 30 days."
                label="About node-hour usage"
              />
            </div>
          </div>
          <LoadingPanel v-if="refreshing && !usageKinds.length" compact message="Loading usage…" />
          <EmptyState
            v-else-if="!usageKinds.length"
            compact
            icon="clock"
            title="No usage recorded yet"
          />
          <DataTable v-else>
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
          </DataTable>
        </div>

        <div class="panel shrink-0">
          <div class="panel-toolbar">
            <div class="flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">AI pricing</h3>
              <HelpBubble
                text="Platform-wide fallback prices as integer micro-USD per million tokens. One million micro-USD equals $1. Provider-reported charges always take precedence; * matches any model for that provider."
                label="About AI pricing"
              />
            </div>
            <button
              v-if="can('billing:manage')"
              class="btn btn-sm"
              type="button"
              :disabled="savingAiRates"
              @click="addAiRate"
            >
              Add rate
            </button>
          </div>
          <EmptyState
            v-if="!aiRates.length"
            compact
            icon="bolt"
            title="No fallback AI rates"
            description="Usage without a provider-reported cost will remain Unpriced."
          />
          <DataTable v-else>
            <thead>
              <tr>
                <th>Provider</th>
                <th>Model</th>
                <th class="text-right">Input (μ$/1M)</th>
                <th class="text-right">Cached (μ$/1M)</th>
                <th class="text-right">Cache create (μ$/1M)</th>
                <th class="text-right">Output (μ$/1M)</th>
                <th class="text-right">Reasoning (μ$/1M)</th>
                <th v-if="can('billing:manage')"></th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(entry, index) in aiRates"
                :key="`${entry.provider}:${entry.model}:${index}`"
              >
                <td><input v-model="entry.provider" :disabled="!can('billing:manage')" /></td>
                <td><input v-model="entry.model" :disabled="!can('billing:manage')" /></td>
                <td v-for="field in aiRateFields" :key="field" class="text-right">
                  <input
                    v-model.number="entry[field]"
                    class="w-24 text-right tabular-nums"
                    type="number"
                    min="0"
                    step="1"
                    :disabled="!can('billing:manage')"
                    :aria-label="`${field} micro-USD per million tokens`"
                  />
                </td>
                <td v-if="can('billing:manage')">
                  <button class="btn btn-ghost btn-sm" type="button" @click="removeAiRate(index)">
                    Remove
                  </button>
                </td>
              </tr>
            </tbody>
          </DataTable>
          <div v-if="can('billing:manage') && aiRates.length" class="mt-2 flex justify-end">
            <button class="btn btn-primary btn-sm" :disabled="savingAiRates" @click="saveAiRates">
              {{ savingAiRates ? "Saving…" : "Save AI pricing" }}
            </button>
          </div>
        </div>

        <div v-if="can('nodes:operate')" class="panel shrink-0">
          <div class="panel-toolbar">
            <div class="flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">Scale a pool</h3>
              <HelpBubble
                text="Set the desired nodes for an organization-owned backend pool and preview its monthly cost."
                label="About scaling a pool"
              />
            </div>
          </div>
          <form class="flex flex-wrap items-end gap-2" @submit.prevent="scale">
            <label class="grid gap-1 text-xs text-fg-muted">
              <span>Backend</span>
              <select v-model="scaleBackend" class="w-auto min-w-36">
                <option value="standalone">standalone</option>
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
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import {
  orgResourcesService,
  type OrgQuota,
  type OrgResourceGroup,
  type OrgUsage,
  type RateCard,
  type AiRateEntry,
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
const rateCard = ref<RateCard>({ entries: [], ai_entries: [] });
const aiRates = ref<AiRateEntry[]>([]);
const savingAiRates = ref(false);
const aiRateFields = [
  "input_microusd_per_million_tokens",
  "cached_input_microusd_per_million_tokens",
  "cache_creation_input_microusd_per_million_tokens",
  "output_microusd_per_million_tokens",
  "reasoning_microusd_per_million_tokens",
] as const;

const scaleBackend = ref("standalone");
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

function addAiRate() {
  aiRates.value.push({
    provider: "",
    model: "*",
    input_microusd_per_million_tokens: 0,
    cached_input_microusd_per_million_tokens: 0,
    cache_creation_input_microusd_per_million_tokens: 0,
    output_microusd_per_million_tokens: 0,
    reasoning_microusd_per_million_tokens: 0,
  });
}

function removeAiRate(index: number) {
  aiRates.value.splice(index, 1);
}

async function saveAiRates() {
  savingAiRates.value = true;

  try {
    rateCard.value = await orgResourcesService.updateAiRateCard(aiRates.value);
    aiRates.value = structuredClone(rateCard.value.ai_entries);
  } finally {
    savingAiRates.value = false;
  }
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
    rateCard.value = await orgResourcesService
      .fetchRateCard()
      .catch(() => ({ entries: [], ai_entries: [] }));
    aiRates.value = structuredClone(rateCard.value.ai_entries);
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
