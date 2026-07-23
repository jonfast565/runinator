<template>
  <div class="flex min-w-0 flex-col gap-2">
    <div class="flex flex-wrap gap-1.5">
      <span
        class="rounded-pill px-2 py-0.5 text-[11px] font-bold"
        :class="overwrites ? 'bg-warning-bg text-warning-fg' : 'bg-success-bg text-success-fg'"
      >
        {{
          overwrites
            ? `Overwrites ${overwriteCount} item${overwriteCount === 1 ? "" : "s"}`
            : "No overwrites"
        }}
      </span>
      <span class="rounded-pill bg-surface-muted px-2 py-0.5 text-[11px] font-bold text-fg-subtle"
        >{{ workflowRows.length }} workflow{{ workflowRows.length === 1 ? "" : "s" }}</span
      >
      <span
        v-if="triggerRows.length"
        class="rounded-pill bg-surface-muted px-2 py-0.5 text-[11px] font-bold text-fg-subtle"
        >{{ triggerRows.length }} trigger{{ triggerRows.length === 1 ? "" : "s" }}</span
      >
      <span
        v-if="settingRows.length"
        class="rounded-pill bg-surface-muted px-2 py-0.5 text-[11px] font-bold text-fg-subtle"
        >{{ settingRows.length }} setting{{ settingRows.length === 1 ? "" : "s" }}</span
      >
    </div>

    <div v-if="!pack" class="text-xs text-fg-muted">Inspect a pack to preview changes.</div>
    <template v-else>
      <section>
        <h4 class="my-1 text-xs text-fg-subtle">Workflows</h4>
        <div v-if="!workflowRows.length" class="text-xs text-fg-muted">No workflows in pack.</div>
        <ul v-else class="m-0 grid list-none gap-0.5 p-0">
          <li
            v-for="row in workflowRows"
            :key="row.name"
            class="flex min-w-0 items-center gap-2 text-xs"
          >
            <span
              class="min-w-16 shrink-0 rounded px-1.5 py-px text-center text-[10px] font-bold tracking-wide uppercase"
              :class="tagClass(row.status)"
              >{{ statusLabel(row.status) }}</span
            >
            <span class="truncate font-semibold text-fg">{{ row.name }}</span>
            <span class="truncate text-fg-muted tabular-nums">
              <template v-if="row.status === 'changed'"
                >v{{ row.previousVersion }} → v{{ row.version }}</template
              >
              <template v-else>v{{ row.version }}</template>
            </span>
          </li>
        </ul>
      </section>

      <section v-if="triggerRows.length">
        <h4 class="my-1 text-xs text-fg-subtle">Triggers</h4>
        <ul class="m-0 grid list-none gap-0.5 p-0">
          <li
            v-for="(row, index) in triggerRows"
            :key="`${row.workflow}-${row.cron}-${index}`"
            class="flex min-w-0 items-center gap-2 text-xs"
          >
            <span
              class="min-w-16 shrink-0 rounded px-1.5 py-px text-center text-[10px] font-bold tracking-wide uppercase"
              :class="tagClass(row.status)"
              >{{ statusLabel(row.status) }}</span
            >
            <span class="truncate font-semibold text-fg">{{ row.workflow }}</span>
            <span class="truncate text-fg-muted tabular-nums">{{ row.cron }}</span>
          </li>
        </ul>
      </section>

      <section v-if="settingRows.length">
        <h4 class="my-1 text-xs text-fg-subtle">Settings</h4>
        <ul class="m-0 grid list-none gap-0.5 p-0">
          <li
            v-for="row in settingRows"
            :key="`${row.kind}:${row.scope}:${row.name}`"
            class="flex min-w-0 items-center gap-2 text-xs"
          >
            <span
              class="min-w-16 shrink-0 rounded px-1.5 py-px text-center text-[10px] font-bold tracking-wide uppercase"
              :class="tagClass(row.status)"
              >{{ statusLabel(row.status) }}</span
            >
            <span class="truncate font-semibold text-fg">{{ row.scope }}.{{ row.name }}</span>
            <span class="truncate text-fg-muted tabular-nums">{{ row.kind }}</span>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { DevPackInspectResult, SettingKind, WorkflowDefinition } from "../../../core/domain/models";

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

function tagClass(status: DiffStatus): string {
  if (status === "added") {
    return "bg-success-bg text-success-fg";
  }

  if (status === "changed") {
    return "bg-warning-bg text-warning-fg";
  }

  return "bg-surface-muted text-fg-muted";
}

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
