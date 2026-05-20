<template>
  <div class="step-editor step-detail">
    <template v-if="node">
      <header class="step-detail-header">
        <div>
          <span class="node-kind">{{ node.kind }}</span>
          <h2>{{ node.id }}</h2>
          <p>{{ headline }}</p>
        </div>
        <button @click="workflows.openStepEditor(workflows.selectedStepId)">Edit</button>
      </header>

      <section v-if="workflows.selectedNodeIssues.length" class="detail-section validation-section">
        <h3>Validation</h3>
        <div class="detail-rows">
          <div v-for="issue in workflows.selectedNodeIssues" :key="issue.message" class="detail-row">
            <span>{{ issue.severity }}</span>
            <strong>{{ issue.message }}</strong>
          </div>
        </div>
      </section>

      <section v-if="taskDraft" class="detail-band">
        <div class="metric">
          <span>Task</span>
          <strong>{{ taskDraft.name || `Task ${taskDraft.id ?? "-"}` }}</strong>
          <small>{{ taskDraft.enabled ? "enabled" : "disabled" }}</small>
        </div>
      </section>

      <section v-for="section in detailSections" :key="section.title" class="detail-section">
        <h3>{{ section.title }}</h3>
        <div v-if="section.items.length" class="detail-grid">
          <div v-for="item in section.items" :key="item.label" class="detail-item">
            <span>{{ item.label }}</span>
            <strong>{{ item.value }}</strong>
          </div>
        </div>
        <div v-if="section.chips.length" class="chip-row">
          <span v-for="chip in section.chips" :key="chip" class="detail-chip">{{ chip }}</span>
        </div>
        <div v-if="section.rows.length" class="detail-rows">
          <div v-for="row in section.rows" :key="row.label + row.value" class="detail-row">
            <span>{{ row.label }}</span>
            <strong>{{ row.value }}</strong>
            <small v-if="row.note">{{ row.note }}</small>
          </div>
        </div>
      </section>

      <section v-if="resultFields.length" class="detail-section">
        <h3>Outputs</h3>
        <div class="chip-row">
          <span v-for="field in resultFields" :key="field" class="detail-chip">{{ field }}</span>
        </div>
      </section>

      <div class="step-summary-actions">
        <button @click="workflows.openStepEditor(workflows.selectedStepId)">Edit</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.duplicateSelectedStep">Duplicate</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Remove</button>
      </div>
    </template>
    <template v-else>
      <div class="empty-detail">
        <h2>No Step Selected</h2>
        <p>Select a node on the graph or add a node from the workflow toolbar.</p>
        <button @click="workflows.addWorkflowNode('task')">Add Task Node</button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useProvidersStore } from "../../stores/providers";
import { useTasksStore } from "../../stores/tasks";
import { useWorkflowsStore } from "../../stores/workflows";
import type { JsonRecord } from "../../types/models";
import { directTransitionKeys, nodeRefId, workflowNodeActionConfig } from "../../utils/workflows";

interface DetailItem {
  label: string;
  value: string;
}

interface DetailRow extends DetailItem {
  note?: string;
}

interface DetailSection {
  title: string;
  items: DetailItem[];
  chips: string[];
  rows: DetailRow[];
}

const workflows = useWorkflowsStore();
const tasksStore = useTasksStore();
const providersStore = useProvidersStore();

const node = computed<JsonRecord | null>(() => workflows.selectedNode);
const taskDraft = computed(() => {
  const current = node.value;
  if (!current || (current.kind !== "task" && current.kind !== "action")) return null;
  return workflows.workflowTaskDrafts[current.id] ?? tasksStore.tasks.find((task) => task.id === Number(current.task_id)) ?? null;
});
const actionConfig = computed(() => (node.value ? workflowNodeActionConfig(node.value, taskDraft.value) : { provider: "", action: "" }));
const provider = computed(() => providersStore.providers.find((item) => item.name === actionConfig.value.provider) ?? null);
const action = computed(() => provider.value?.actions.find((item) => item.function_name === actionConfig.value.action) ?? null);

const headline = computed(() => {
  const current = node.value;
  if (!current) return "";
  switch (current.kind) {
    case "action":
    case "task":
      return actionConfig.value.provider ? `${actionConfig.value.provider} · ${actionConfig.value.action || "action"}` : "Unconfigured action";
    case "approval":
      return String(current.parameters?.prompt ?? "Approval required");
    case "condition":
      return `${branchRows(current).length} conditional route${branchRows(current).length === 1 ? "" : "s"}`;
    case "wait":
      return waitSummary(current.wait);
    case "start":
      return "Workflow entry point";
    case "end":
      return "Terminal workflow step";
    case "fail":
      return "Terminal failure step";
    default:
      return `${current.kind} control node`;
  }
});

const resultFields = computed(() =>
  (action.value?.results ?? []).map((result) => `${result.label || result.name}: ${result.value_type}`)
);

const detailSections = computed<DetailSection[]>(() => {
  const current = node.value;
  if (!current) return [];
  const sections = [kindSection(current), transitionsSection(current)].filter(Boolean) as DetailSection[];
  return sections.filter((section) => section.items.length || section.chips.length || section.rows.length);
});

function kindSection(current: JsonRecord): DetailSection {
  switch (current.kind) {
    case "action":
    case "task":
      return taskSection(current);
    case "approval":
      return section("Approval", [
        item("Type", current.parameters?.approval_type ?? current.parameters?.type ?? "generic"),
        item("Prompt", current.parameters?.prompt ?? "Approval required")
      ]);
    case "condition":
      return section("Conditions", [], [], branchRows(current));
    case "wait":
      return section("Wait", waitItems(current.wait));
    case "loop":
      return section("Loop", [
        item("Items", valueLabel(current.parameters?.items)),
        item("Target", refLabel(current.parameters?.target)),
        item("Max Iterations", current.max_iterations ?? 10)
      ]);
    case "switch":
      return section("Switch", [item("Value", valueLabel(current.parameters?.value))], [], switchRows(current));
    case "parallel":
      return section("Parallel", [], nodeRefArray(current.parameters?.branches).map((target) => `branch -> ${target}`));
    case "join":
      return section("Join", [item("Mode", current.parameters?.mode ?? "all")], nodeRefArray(current.parameters?.wait_for).map((target) => `wait for ${target}`));
    case "try":
      return section("Try", [
        item("Body", refLabel(current.parameters?.body)),
        item("Catch", refLabel(current.parameters?.catch)),
        item("Finally", refLabel(current.parameters?.finally))
      ]);
    case "map":
      return section("Map", [
        item("Items", valueLabel(current.parameters?.items)),
        item("Target", refLabel(current.parameters?.target)),
        item("Concurrency", current.parameters?.concurrency ?? "-")
      ]);
    case "race":
      return section("Race", [item("Winner", current.parameters?.winner ?? "first_success")], nodeRefArray(current.parameters?.branches).map((target) => `race -> ${target}`));
    case "emit":
      return section("Emit", [
        item("Event", current.parameters?.event_type ?? "workflow.event"),
        item("Data", valueLabel(current.parameters?.data))
      ]);
    case "subflow":
      return section("Subflow", [
        item("Workflow ID", current.subflow_id ?? "-"),
        item("Parameters", valueLabel(current.parameters))
      ]);
    case "start":
      return section("Start", [item("Starts At", refLabel(current.transitions?.next))]);
    case "end":
      return section("End", [item("Terminal", "yes")]);
    case "fail":
      return section("Fail", [item("Terminal", "yes")]);
    default:
      return section(String(current.kind ?? "Node"), [item("Parameters", valueLabel(current.parameters))]);
  }
}

function taskSection(current: JsonRecord): DetailSection {
  const task = taskDraft.value;
  if (!task) {
    return section("Action", [
      item("Provider", actionConfig.value.provider || "-"),
      item("Action", actionConfig.value.action || "-"),
      item("Timeout", `${current.timeout_seconds ?? current.action?.timeout_seconds ?? "-"}s`),
      item("Retries", current.retry?.max_attempts ?? 1),
      item("Step Parameters", valueLabel(current.parameters))
    ]);
  }
  return section(
    current.kind === "action" ? "Action" : "Task",
    [
      item("Name", task.name || "-"),
      item("Provider", actionConfig.value.provider || "-"),
      item("Action", actionConfig.value.action || "-"),
      item("Schedule", task.cron_schedule || "-"),
      item("Timeout", `${task.timeout}s`),
      item("Retries", current.retry?.max_attempts ?? 1),
      item("Step Parameters", valueLabel(current.parameters))
    ],
    [
      task.enabled ? "scheduled" : "workflow-only",
      task.mcp_enabled ? "mcp" : "",
      ...(action.value?.parameters ?? []).filter((param) => param.required).map((param) => `requires ${param.name}`)
    ].filter(Boolean)
  );
}

function transitionsSection(current: JsonRecord): DetailSection {
  const transitions = current.transitions ?? {};
  const rows: DetailRow[] = [];
  for (const key of directTransitionKeys) {
    const target = nodeRefId(transitions[key]);
    if (target) rows.push({ label: key, value: target });
  }
  if (Array.isArray(transitions.branches)) {
    transitions.branches.forEach((branch: JsonRecord, index: number) => {
      const target = nodeRefId(branch.target);
      if (target) rows.push({ label: branch.label ?? `branch ${index + 1}`, value: target, note: conditionLabel(branch.when) });
    });
  }
  return section("Transitions", [], [], rows);
}

function branchRows(current: JsonRecord): DetailRow[] {
  const branches = Array.isArray(current.transitions?.branches) ? current.transitions.branches : [];
  return branches.map((branch: JsonRecord, index: number) => ({
    label: branch.label ?? `branch ${index + 1}`,
    value: refLabel(branch.target),
    note: conditionLabel(branch.when)
  }));
}

function switchRows(current: JsonRecord): DetailRow[] {
  const cases = Array.isArray(current.parameters?.cases) ? current.parameters.cases : [];
  const rows = cases.map((switchCase: JsonRecord, index: number) => ({
    label: switchCase.label ?? `case ${index + 1}`,
    value: refLabel(switchCase.target),
    note: conditionLabel(switchCase.when ?? switchCase.condition)
  }));
  if (current.parameters?.default) rows.push({ label: "default", value: refLabel(current.parameters.default) });
  return rows;
}

function waitItems(wait: unknown): DetailItem[] {
  const record = isRecord(wait) ? wait : {};
  return [
    item("Seconds", record.seconds ?? "-"),
    item("Until", record.until ?? "-")
  ];
}

function nodeRefArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map(nodeRefId).filter((target): target is string => Boolean(target)) : [];
}

function conditionLabel(value: unknown): string {
  if (!isRecord(value)) return valueLabel(value);
  if ("equals" in value) return `${valueLabel(value.value)} equals ${valueLabel(value.equals)}`;
  if ("not_equals" in value) return `${valueLabel(value.value)} not equals ${valueLabel(value.not_equals)}`;
  if ("exists" in value) return `${valueLabel(value.exists)} exists`;
  return valueLabel(value);
}

function refLabel(value: unknown): string {
  return nodeRefId(value) ?? "-";
}

function valueLabel(value: unknown): string {
  if (value == null) return "-";
  if (typeof value === "string") return value || "-";
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.length ? value.map(valueLabel).join(", ") : "empty list";
  if (!isRecord(value)) return String(value);
  if (nodeRefId(value)) return `node ${nodeRefId(value)}`;
  if (isRecord(value.$ref)) return refExpressionLabel(value.$ref);
  if (Array.isArray(value.$concat)) return `concat ${value.$concat.length} part${value.$concat.length === 1 ? "" : "s"}`;
  const entries = Object.entries(value);
  if (entries.length === 0) return "none";
  return entries.slice(0, 4).map(([key, nested]) => `${key}: ${valueLabel(nested)}`).join("; ") + (entries.length > 4 ? `; +${entries.length - 4} more` : "");
}

function refExpressionLabel(ref: JsonRecord): string {
  for (const source of ["input", "prev", "workflow", "output"]) {
    if (Array.isArray(ref[source])) return `${source}.${ref[source].join(".")}`;
  }
  if (typeof ref.node === "string" && Array.isArray(ref.output)) return `${ref.node}.output.${ref.output.join(".")}`;
  return "reference";
}

function waitSummary(wait: unknown): string {
  const record = isRecord(wait) ? wait : {};
  if (record.seconds) return `Wait ${record.seconds}s`;
  if (record.until) return `Wait until ${record.until}`;
  return "Wait for external timing";
}

function item(label: string, raw: unknown): DetailItem {
  return { label, value: valueLabel(raw) };
}

function section(title: string, items: DetailItem[] = [], chips: string[] = [], rows: DetailRow[] = []): DetailSection {
  return { title, items: items.filter((entry) => entry.value !== "-"), chips, rows };
}

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
</script>

<style scoped>
.step-detail {
  gap: 12px;
  padding: 12px;
}

.step-detail-header,
.step-summary-actions,
.detail-band {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.step-detail-header h2,
.step-detail-header p {
  margin: 0;
}

.node-kind {
  color: #66717e;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
}

.step-detail-header p,
.detail-item span,
.detail-row span,
.metric span,
.metric small,
.empty-detail p {
  color: #66717e;
}

.detail-band {
  border: 1px solid #dbe5ef;
  border-radius: 6px;
  background: #f8fafc;
  padding: 10px;
}

.metric {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.metric strong,
.detail-item strong,
.detail-row strong {
  min-width: 0;
  overflow: hidden;
  color: #17202a;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-section {
  display: grid;
  gap: 8px;
  border-top: 1px solid #e5ebf1;
  padding-top: 10px;
}

.detail-section h3 {
  margin: 0;
  color: #17202a;
  font-size: 13px;
}

.detail-grid {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.detail-item,
.detail-row {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.detail-item span,
.detail-row span {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
}

.detail-row {
  border-left: 3px solid #dbe5ef;
  padding-left: 8px;
}

.detail-row small {
  overflow: hidden;
  color: #4b5663;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-rows,
.chip-row {
  display: grid;
  gap: 6px;
}

.chip-row {
  display: flex;
  flex-wrap: wrap;
}

.detail-chip {
  border: 1px solid #dbe5ef;
  border-radius: 999px;
  background: #ffffff;
  color: #4b5663;
  font-size: 12px;
  padding: 3px 8px;
}

.empty-detail {
  display: grid;
  gap: 8px;
  padding: 12px;
}
</style>
