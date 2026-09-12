<template>
  <section class="pane">
    <div class="panel">
      <PanelHeader
        title="Capability Coverage"
        icon="shield"
        eyebrow="Command Center accessibility contract"
        description="Audit every operator capability for discovery, typed configuration, runtime evidence, safe control, and an explicit boundary. New operator APIs cannot pass CI without joining this inventory."
      />

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <MetricCard label="Capabilities" :value="capabilities.length" />
        <MetricCard label="Requirements answered" :value="answeredCount" />
        <MetricCard label="Open gaps" :value="gapCount" />
      </div>

      <div class="mt-3 flex flex-wrap items-center gap-2">
        <label class="text-xs text-fg-muted" for="coverage-area">Area</label>
        <select id="coverage-area" v-model="selectedArea" class="max-w-xs">
          <option value="">All areas</option>
          <option v-for="area in areas" :key="area" :value="area">{{ area }}</option>
        </select>
        <label class="ml-auto inline-flex items-center gap-1.5 text-xs text-fg-muted">
          <input v-model="gapsOnly" type="checkbox" class="w-auto" />
          Show gaps only
        </label>
      </div>
    </div>

    <div v-if="!filteredCapabilities.length" class="panel">
      <EmptyState
        compact
        icon="check"
        title="No capability gaps"
        description="Every capability in this scope has an exposed surface or a recorded intentional boundary."
      />
    </div>

    <div v-else class="grid gap-3 xl:grid-cols-2">
      <article
        v-for="capability in filteredCapabilities"
        :key="capability.id"
        class="panel min-w-0"
      >
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted">{{
              capability.area
            }}</span>
            <h2 class="mt-1 mb-0 text-base font-semibold text-fg">{{ capability.label }}</h2>
            <p class="mt-1 mb-0 text-sm text-fg-subtle">{{ capability.summary }}</p>
          </div>
          <span
            class="badge shrink-0"
            :class="
              capabilityHasGap(capability)
                ? 'bg-danger-bg text-danger-fg'
                : 'bg-success-bg text-success-fg'
            "
          >
            {{ capabilityHasGap(capability) ? "Gap" : "Covered" }}
          </span>
        </div>

        <dl class="mt-3 grid gap-2">
          <div
            v-for="requirement in coverageRequirements"
            :key="requirement"
            class="grid gap-1 rounded-md border border-border-subtle bg-surface-muted px-3 py-2 sm:grid-cols-[112px_90px_1fr] sm:items-start"
          >
            <dt class="text-xs font-semibold capitalize text-fg">{{ requirement }}</dt>
            <dd class="m-0">
              <span class="badge" :class="outcomeClass(capability.coverage[requirement].outcome)">
                {{ coverageOutcomeLabel(capability.coverage[requirement].outcome) }}
              </span>
            </dd>
            <dd class="m-0 text-xs leading-relaxed text-fg-subtle">
              {{ capability.coverage[requirement].detail }}
            </dd>
          </div>
        </dl>

        <div class="mt-3 flex flex-wrap gap-2">
          <button
            v-for="tab in capability.tabs"
            :key="tab"
            type="button"
            class="btn btn-sm"
            @click="app.activeTab = tab"
          >
            Open {{ navItemForTab(tab)?.label ?? tab }}
          </button>
        </div>
      </article>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import {
  capabilityHasGap,
  controlSurfaceCoverage,
  coverageOutcomeLabel,
  coverageRequirements,
  type CoverageOutcome,
} from "../../core/services/control-surface-coverage";
import { navItemForTab } from "../../core/navigation/nav-config";
import { useAppStore } from "../adapters/pinia/app";
import EmptyState from "../components/shared/EmptyState.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";

const app = useAppStore();
const selectedArea = ref("");
const gapsOnly = ref(false);
const capabilities = controlSurfaceCoverage.capabilities;
const areas = [...new Set(capabilities.map((capability) => capability.area))];
const gapCount = computed(
  () => capabilities.filter((capability) => capabilityHasGap(capability)).length,
);
const answeredCount = capabilities.length * coverageRequirements.length;
const filteredCapabilities = computed(() =>
  capabilities.filter((capability) => {
    const searchable = [
      capability.label,
      capability.area,
      capability.summary,
      ...Object.values(capability.coverage).map((answer) => answer.detail),
    ]
      .join(" ")
      .toLowerCase();
    return (
      (!selectedArea.value || capability.area === selectedArea.value) &&
      (!gapsOnly.value || capabilityHasGap(capability)) &&
      (!app.normalizedSearch || searchable.includes(app.normalizedSearch))
    );
  }),
);

function outcomeClass(outcome: CoverageOutcome): string {
  if (outcome === "gap") {
    return "bg-danger-bg text-danger-fg";
  }

  if (outcome === "intentional") {
    return "bg-info-bg text-info-fg";
  }

  return "bg-success-bg text-success-fg";
}
</script>
