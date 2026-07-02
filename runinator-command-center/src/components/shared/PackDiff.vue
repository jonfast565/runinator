<template>
  <div class="pack-diff">
    <div class="pd-summary">
      <span :class="['pd-pill', overwrites ? 'warn' : 'ok']">
        {{
          overwrites
            ? `Overwrites ${overwriteCount} item${overwriteCount === 1 ? "" : "s"}`
            : "No overwrites"
        }}
      </span>
      <span class="pd-pill muted"
        >{{ workflowRows.length }} workflow{{ workflowRows.length === 1 ? "" : "s" }}</span
      >
      <span v-if="triggerRows.length" class="pd-pill muted"
        >{{ triggerRows.length }} trigger{{ triggerRows.length === 1 ? "" : "s" }}</span
      >
      <span v-if="settingRows.length" class="pd-pill muted"
        >{{ settingRows.length }} setting{{ settingRows.length === 1 ? "" : "s" }}</span
      >
    </div>

    <div v-if="!pack" class="pd-empty">Inspect a pack to preview changes.</div>
    <template v-else>
      <section class="pd-section">
        <h4>Workflows</h4>
        <div v-if="!workflowRows.length" class="pd-none">No workflows in pack.</div>
        <ul v-else class="pd-list">
          <li v-for="row in workflowRows" :key="row.name">
            <span class="pd-tag" :class="row.status">{{ statusLabel(row.status) }}</span>
            <span class="pd-name">{{ row.name }}</span>
            <span v-if="row.status === 'changed'" class="pd-detail"
              >v{{ row.previousVersion }} → v{{ row.version }}</span
            >
            <span v-else class="pd-detail">v{{ row.version }}</span>
          </li>
        </ul>
      </section>

      <section v-if="triggerRows.length" class="pd-section">
        <h4>Triggers</h4>
        <ul class="pd-list">
          <li v-for="(row, index) in triggerRows" :key="`${row.workflow}-${row.cron}-${index}`">
            <span class="pd-tag" :class="row.status">{{ statusLabel(row.status) }}</span>
            <span class="pd-name">{{ row.workflow }}</span>
            <span class="pd-detail">{{ row.cron }}</span>
          </li>
        </ul>
      </section>

      <section v-if="settingRows.length" class="pd-section">
        <h4>Settings</h4>
        <ul class="pd-list">
          <li v-for="row in settingRows" :key="`${row.kind}:${row.scope}:${row.name}`">
            <span class="pd-tag" :class="row.status">{{ statusLabel(row.status) }}</span>
            <span class="pd-name">{{ row.scope }}.{{ row.name }}</span>
            <span class="pd-detail">{{ row.kind }}</span>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { DevPackInspectResult, SettingKind, WorkflowDefinition } from "../../types/models";

type DiffStatus = "added" | "changed" | "unchanged";

interface WorkflowRow {
  name: string;
  status: DiffStatus;
  version: string;
  previousVersion: string;
}
interface TriggerRow {
  workflow: string;
  cron: string;
  status: DiffStatus;
}
interface SettingRow {
  scope: string;
  name: string;
  kind: SettingKind;
  status: DiffStatus;
}

const props = defineProps<{
  pack: DevPackInspectResult | null;
  existingWorkflows: WorkflowDefinition[];
  existingSettings: { scope: string; name: string; kind?: SettingKind }[];
}>();

const existingByName = computed(() => {
  const map = new Map<string, WorkflowDefinition>();

  for (const workflow of props.existingWorkflows) {
    map.set(workflow.name, workflow);
  }

  return map;
});

const existingSettingKeys = computed(() => {
  const set = new Set<string>();

  for (const setting of props.existingSettings) {
    set.add(`${setting.kind ?? "secret"}:${setting.scope}:${setting.name}`);
  }

  return set;
});

const workflowRows = computed<WorkflowRow[]>(() => {
  const rows: WorkflowRow[] = [];

  for (const workflow of props.pack?.workflows ?? []) {
    const existing = existingByName.value.get(workflow.name);
    let status: DiffStatus = "added";

    if (existing) {
      status = workflowsEqual(workflow, existing) ? "unchanged" : "changed";
    }

    rows.push({
      name: workflow.name,
      status,
      version: workflow.version,
      previousVersion: existing?.version ?? workflow.version,
    });
  }

  return rows.sort(
    (left, right) =>
      statusRank(left.status) - statusRank(right.status) || left.name.localeCompare(right.name),
  );
});

const settingRows = computed<SettingRow[]>(() => {
  const rows: SettingRow[] = [];

  for (const setting of props.pack?.settings ?? []) {
    const key = `${setting.kind}:${setting.scope}:${setting.name}`;
    rows.push({
      scope: setting.scope,
      name: setting.name,
      kind: setting.kind,
      // the server does not expose stored values, so an existing slot is reported as overwritten.
      status: existingSettingKeys.value.has(key) ? "changed" : "added",
    });
  }

  return rows.sort(
    (left, right) =>
      statusRank(left.status) - statusRank(right.status) || left.scope.localeCompare(right.scope),
  );
});

const triggerRows = computed<TriggerRow[]>(() => {
  const rows: TriggerRow[] = [];

  for (const workflow of props.pack?.workflows ?? []) {
    // a workflow that does not yet exist contributes added triggers; an existing one has them replaced.
    const status: DiffStatus = existingByName.value.has(workflow.name) ? "changed" : "added";

    for (const cron of cronExpressionsFor(workflow)) {
      rows.push({ workflow: workflow.name, cron, status });
    }
  }

  return rows;
});

const overwriteCount = computed(
  () =>
    workflowRows.value.filter((row) => row.status === "changed").length +
    settingRows.value.filter((row) => row.status === "changed").length,
);
const overwrites = computed(() => overwriteCount.value > 0);

function statusRank(status: DiffStatus): number {
  return status === "changed" ? 0 : status === "added" ? 1 : 2;
}

function statusLabel(status: DiffStatus): string {
  if (status === "added") {
    return "new";
  }

  if (status === "changed") {
    return "overwrite";
  }

  return "unchanged";
}

// compare the importable shape of a workflow; node ordering is normalized via canonical json.
function workflowsEqual(left: WorkflowDefinition, right: WorkflowDefinition): boolean {
  return (
    left.version === right.version &&
    left.enabled === right.enabled &&
    canonical(left.input_type) === canonical(right.input_type) &&
    canonical(left.definition) === canonical(right.definition)
  );
}

// stable stringify so key ordering differences do not register as changes.
function canonical(value: unknown): string {
  return JSON.stringify(sortKeys(value));
}

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortKeys);
  }

  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>).sort(([a], [b]) =>
      a.localeCompare(b),
    );
    return Object.fromEntries(entries.map(([key, val]) => [key, sortKeys(val)]));
  }

  return value;
}

// pull cron expressions a pack workflow declares via its wdl header (definition.metadata.triggers).
function cronExpressionsFor(workflow: WorkflowDefinition): string[] {
  const triggers = (workflow.definition.metadata as Record<string, unknown> | undefined)?.triggers;

  if (!Array.isArray(triggers)) {
    return [];
  }

  const crons: string[] = [];

  for (const trigger of triggers) {
    if (typeof trigger === "string") {
      crons.push(trigger);
    } else if (trigger && typeof trigger === "object") {
      const record = trigger as Record<string, unknown>;
      const configuration = record.configuration;
      const configCron =
        configuration && typeof configuration === "object"
          ? (configuration as Record<string, unknown>).cron
          : undefined;
      const cron = record.cron ?? record.expression ?? configCron;

      if (typeof cron === "string") {
        crons.push(cron);
      }
    }
  }

  return crons;
}
</script>

<style scoped>
.pack-diff {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.pd-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.pd-pill {
  border-radius: 999px;
  padding: 2px 9px;
  font-size: 11px;
  font-weight: 700;
}
.pd-pill.ok {
  background: var(--success-bg);
  color: var(--success-fg);
}
.pd-pill.warn {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
.pd-pill.muted {
  background: var(--surface-muted);
  color: var(--text-subtle);
}
.pd-empty,
.pd-none {
  color: var(--text-muted);
  font-size: 12px;
}
.pd-section h4 {
  margin: 4px 0 4px;
  font-size: 12px;
  color: var(--text-subtle);
}
.pd-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 3px;
}
.pd-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  min-width: 0;
}
.pd-tag {
  flex: 0 0 auto;
  min-width: 64px;
  text-align: center;
  border-radius: 4px;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}
.pd-tag.added {
  background: var(--success-bg);
  color: var(--success-fg);
}
.pd-tag.changed {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
.pd-tag.unchanged {
  background: var(--surface-muted);
  color: var(--text-muted);
}
.pd-name {
  font-weight: 600;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pd-detail {
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
