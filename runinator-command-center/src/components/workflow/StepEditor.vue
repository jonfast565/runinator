<template>
  <div class="step-editor step-detail">
    <template v-if="node">
      <header class="step-detail-header">
        <div class="step-detail-heading">
          <div class="step-detail-titles">
            <span class="node-kind">{{ node.kind }}</span>
            <h2 :title="String(node.id)">{{ node.id }}</h2>
            <span v-if="displayName" class="step-detail-name">{{ displayName }}</span>
          </div>
          <div v-if="flags.length" class="flag-row">
            <span v-for="flag in flags" :key="flag.label" class="step-flag" :class="`flag-${flag.tone}`">{{ flag.label }}</span>
          </div>
          <p class="step-headline">{{ headline }}</p>
        </div>
        <button class="step-edit-btn" @click="workflows.openStepEditor(workflows.selectedStepId)">Edit</button>
      </header>

      <section v-if="workflows.selectedNodeIssues.length" class="detail-section validation-section">
        <h3>Validation</h3>
        <div class="detail-rows">
          <div v-for="issue in workflows.selectedNodeIssues" :key="issue.message" class="detail-row" :class="`issue-${issue.severity}`">
            <span>{{ issue.severity }}</span>
            <strong>{{ issue.message }}</strong>
          </div>
        </div>
      </section>

      <!-- action steps get a dedicated, structured breakdown of provider, parameters, and outputs. -->
      <template v-if="node.kind === 'action'">
        <section class="detail-section">
          <h3>Action</h3>
          <p v-if="actionDescription" class="section-note">{{ actionDescription }}</p>
          <div class="detail-grid">
            <div v-for="entry in actionMeta" :key="entry.label" class="detail-item">
              <span>{{ entry.label }}</span>
              <strong :class="{ mono: entry.mono }">{{ entry.value }}</strong>
            </div>
          </div>
          <p v-if="!action && actionConfig.provider" class="section-note warn">
            Provider “{{ actionConfig.provider }}” is not registered; showing the raw configuration.
          </p>
        </section>

        <section class="detail-section">
          <h3>Parameters <span class="count-pill">{{ paramRows.length }}</span></h3>
          <div v-if="paramRows.length" class="param-list">
            <div v-for="param in paramRows" :key="param.name" class="param-item" :class="{ unset: !param.configured }">
              <div class="param-head">
                <code class="param-name">{{ param.name }}</code>
                <span v-if="param.type" class="param-type">{{ param.type }}</span>
                <span v-if="param.required" class="param-tag tag-req">required</span>
                <span v-if="param.secret" class="param-tag tag-secret">secret</span>
              </div>
              <div class="param-value" :class="{ muted: !param.configured }">{{ param.value }}</div>
              <p v-if="param.description" class="param-desc">{{ param.description }}</p>
            </div>
          </div>
          <p v-else class="empty-note">No parameters defined for this action.</p>
        </section>

        <section v-if="resultRows.length" class="detail-section">
          <h3>Outputs <span class="count-pill">{{ resultRows.length }}</span></h3>
          <div class="param-list">
            <div v-for="result in resultRows" :key="result.name" class="param-item">
              <div class="param-head">
                <code class="param-name">{{ result.name }}</code>
                <span v-if="result.type" class="param-type">{{ result.type }}</span>
              </div>
              <p v-if="result.description" class="param-desc">{{ result.description }}</p>
            </div>
          </div>
        </section>
      </template>

      <!-- control nodes keep the compact summary grid keyed off their kind. -->
      <section v-for="sect in detailSections" v-else :key="sect.title" class="detail-section">
        <h3>{{ sect.title }}</h3>
        <div v-if="sect.items.length" class="detail-grid">
          <div v-for="entry in sect.items" :key="entry.label" class="detail-item">
            <span>{{ entry.label }}</span>
            <strong>{{ entry.value }}</strong>
          </div>
        </div>
        <div v-if="sect.chips.length" class="chip-row">
          <span v-for="chip in sect.chips" :key="chip" class="detail-chip">{{ chip }}</span>
        </div>
        <div v-if="sect.rows.length" class="detail-rows">
          <div v-for="row in sect.rows" :key="row.label + row.value" class="detail-row">
            <span>{{ row.label }}</span>
            <strong class="mono">{{ row.value }}</strong>
            <small v-if="row.note">{{ row.note }}</small>
          </div>
        </div>
      </section>

      <section class="detail-section">
        <h3>Transitions</h3>
        <div v-if="transitionRows.length" class="detail-rows">
          <div v-for="row in transitionRows" :key="row.label + row.value" class="detail-row transition-row">
            <span>{{ row.label }}</span>
            <strong class="mono">{{ row.value }}</strong>
            <small v-if="row.note">{{ row.note }}</small>
          </div>
        </div>
        <p v-else class="empty-note">No outgoing transitions.</p>
      </section>

      <div class="step-summary-actions">
        <button class="primary" @click="workflows.openStepEditor(workflows.selectedStepId)">Edit</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.duplicateSelectedStep">Duplicate</button>
        <button :disabled="!workflows.canRemoveSelectedStep" @click="workflows.removeWorkflowStep">Remove</button>
      </div>
    </template>
    <template v-else>
      <div class="empty-detail">
        <h2>No Step Selected</h2>
        <p>Select a node on the graph or add a node from the workflow toolbar.</p>
        <button @click="workflows.addWorkflowNode('action')">Add Node</button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useProvidersStore } from "../../stores/providers";
import { useWorkflowsStore } from "../../stores/workflows";
import type { JsonRecord, RuninatorType } from "../../types/models";
import { directTransitionKeys, nodeRefId, workflowNodeActionConfig, workflowNodeActionInputs } from "../../utils/workflows";

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

interface MetaEntry {
  label: string;
  value: string;
  mono?: boolean;
}

interface ParamRow {
  name: string;
  type: string;
  required: boolean;
  secret: boolean;
  value: string;
  description: string;
  configured: boolean;
}

interface ResultRow {
  name: string;
  type: string;
  description: string;
}

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();

const node = computed<JsonRecord | null>(() => workflows.selectedNode);
const actionConfig = computed(() => (node.value ? workflowNodeActionConfig(node.value) : { provider: "", action: "" }));
const provider = computed(() => providersStore.providers.find((item) => item.name === actionConfig.value.provider) ?? null);
const action = computed(() => provider.value?.actions.find((item) => item.function_name === actionConfig.value.action) ?? null);

// the human label shown on the node, only when it differs from the id.
const displayName = computed(() => {
  const name = node.value?.name;
  return typeof name === "string" && name && name !== node.value?.id ? name : "";
});

const flags = computed<{ label: string; tone: string }[]>(() => {
  const current = node.value;
  if (!current) return [];
  const out: { label: string; tone: string }[] = [];
  if (current.locked) out.push({ label: "locked", tone: "neutral" });
  if (current.skipped) out.push({ label: "skipped", tone: "warn" });
  if (current.kind === "action" && current.run_once) out.push({ label: "run once", tone: "neutral" });
  return out;
});

const actionDescription = computed(() => action.value?.description ?? "");

const headline = computed(() => {
  const current = node.value;
  if (!current) return "";
  switch (current.kind) {
    case "action":
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

// action header band: provider, function, timeout, retries.
const actionMeta = computed<MetaEntry[]>(() => {
  const current = node.value;
  if (!current || current.kind !== "action") return [];
  const retries = current.retry?.max_attempts ?? current.max_attempts ?? 1;
  const timeout = current.timeout_seconds ?? current.action?.timeout_seconds;
  return [
    { label: "Provider", value: actionConfig.value.provider || "—", mono: true },
    { label: "Function", value: actionConfig.value.action || "—", mono: true },
    { label: "Timeout", value: timeout != null ? `${timeout}s` : "default" },
    { label: "Max Attempts", value: String(retries) }
  ];
});

// one row per provider parameter, merged with the value configured on this node.
const paramRows = computed<ParamRow[]>(() => {
  const current = node.value;
  if (!current || current.kind !== "action") return [];
  const inputs = workflowNodeActionInputs(current);
  const inputRecord = isRecord(inputs) ? inputs : {};
  const schema = action.value?.parameters ?? [];

  if (schema.length) {
    return schema.map((param) => {
      const configured = param.name in inputRecord;
      const value = configured
        ? valueLabel(inputRecord[param.name])
        : param.default_value != null
          ? `${valueLabel(param.default_value)} · default`
          : "not set";
      return {
        name: param.name,
        type: renderType(param.ty),
        required: param.required,
        secret: param.secret,
        value,
        description: param.description ?? "",
        configured
      };
    });
  }

  // unknown provider: surface whatever inputs are actually set so detail is not lost.
  return Object.entries(inputRecord).map(([name, raw]) => ({
    name,
    type: "",
    required: false,
    secret: false,
    value: valueLabel(raw),
    description: "",
    configured: true
  }));
});

const resultRows = computed<ResultRow[]>(() =>
  (action.value?.results ?? []).map((result) => ({
    name: result.label || result.name,
    type: renderType(result.ty),
    description: result.description ?? ""
  }))
);

const detailSections = computed<DetailSection[]>(() => {
  const current = node.value;
  if (!current) return [];
  return [kindSection(current)].filter((section) => section.items.length || section.chips.length || section.rows.length);
});

const transitionRows = computed<DetailRow[]>(() => (node.value ? transitionsSection(node.value).rows : []));

function kindSection(current: JsonRecord): DetailSection {
  switch (current.kind) {
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
    case "config":
      return section("Config", [
        item("Name", valueLabel(current.parameters?.name)),
        item("Metadata", valueLabel(current.parameters?.metadata))
      ]);
    case "subflow":
      return section("Subflow", [
        item("Workflow", subflowLabel(current.subflow_id)),
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

// prefer the target workflow's name over its raw id, falling back to the id when unresolved.
function subflowLabel(subflowId: unknown): string {
  const id = subflowId != null ? String(subflowId) : "";
  if (!id) return "-";
  const name = workflows.workflows.find((workflow) => workflow.id === id)?.name;
  return name || `Workflow ${id}`;
}

// render a runinator type into a short readable signature (e.g. array<string>, map<integer>).
function renderType(ty: RuninatorType | null | undefined): string {
  if (!ty) return "any";
  switch (ty.type) {
    case "array":
      return `array<${renderType(ty.items)}>`;
    case "map":
      return `map<${renderType(ty.values)}>`;
    case "struct":
      return "struct";
    case "union":
      return ty.variants.map(renderType).join(" | ");
    default:
      return ty.type;
  }
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
  gap: 14px;
  padding: 14px;
}

.step-detail-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.step-detail-heading {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.step-detail-titles {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px 8px;
  min-width: 0;
}

.step-detail-titles h2 {
  margin: 0;
  font-size: 17px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.step-detail-name {
  color: #4b5663;
  font-size: 13px;
}

.node-kind {
  color: #5560d8;
  background: #eef0ff;
  border-radius: 4px;
  padding: 2px 7px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.step-headline {
  margin: 0;
  color: #66717e;
  font-size: 12px;
}

.flag-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.step-flag {
  border-radius: 999px;
  padding: 1px 8px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.flag-neutral {
  background: #eef2f7;
  color: #55606d;
}

.flag-warn {
  background: #fff4cc;
  color: #8a5a00;
}

.step-edit-btn {
  flex: 0 0 auto;
}

.detail-section {
  display: grid;
  gap: 8px;
  border-top: 1px solid #e5ebf1;
  padding-top: 12px;
}

.detail-section h3 {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0;
  color: #17202a;
  font-size: 13px;
}

.count-pill {
  color: #66717e;
  background: #eef2f7;
  border-radius: 999px;
  padding: 0 7px;
  font-size: 11px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.section-note {
  margin: -2px 0 0;
  color: #5b6675;
  font-size: 12px;
  line-height: 1.45;
}

.section-note.warn {
  color: #8a5a00;
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
  color: #66717e;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.03em;
  text-transform: uppercase;
}

.detail-item strong,
.detail-row strong {
  min-width: 0;
  overflow: hidden;
  color: #17202a;
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.detail-rows {
  display: grid;
  gap: 6px;
}

.detail-row {
  border-left: 3px solid #dbe5ef;
  padding-left: 8px;
}

.detail-row.transition-row {
  border-left-color: #c4d4ea;
}

.detail-row.issue-error {
  border-left-color: #dc2626;
}

.detail-row.issue-warning {
  border-left-color: #d97706;
}

.detail-row small {
  overflow: hidden;
  color: #4b5663;
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* parameter / output list: one card per field with type and value. */
.param-list {
  display: grid;
  gap: 8px;
}

.param-item {
  display: grid;
  gap: 4px;
  border: 1px solid #e5ebf1;
  border-radius: 6px;
  background: #fbfcfe;
  padding: 8px 10px;
}

.param-item.unset {
  background: #fafbfc;
  border-style: dashed;
}

.param-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.param-name {
  color: #17202a;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  font-weight: 600;
}

.param-type {
  color: #5560d8;
  background: #eef0ff;
  border-radius: 4px;
  padding: 0 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
}

.param-tag {
  border-radius: 999px;
  padding: 0 7px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.tag-req {
  color: #b42318;
  background: #fee4e2;
}

.tag-secret {
  color: #6941c6;
  background: #f4ebff;
}

.param-value {
  color: #1f2937;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
  word-break: break-word;
}

.param-value.muted {
  color: #97a1ad;
  font-style: italic;
}

.param-desc {
  margin: 0;
  color: #66717e;
  font-size: 11px;
  line-height: 1.45;
}

.chip-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.detail-chip {
  border: 1px solid #dbe5ef;
  border-radius: 999px;
  background: #ffffff;
  color: #4b5663;
  font-size: 12px;
  padding: 3px 8px;
}

.empty-note {
  margin: 0;
  color: #97a1ad;
  font-size: 12px;
}

.step-summary-actions {
  display: flex;
  gap: 8px;
  border-top: 1px solid #e5ebf1;
  padding-top: 12px;
}

.empty-detail {
  display: grid;
  gap: 8px;
  padding: 12px;
}

.empty-detail p {
  color: #66717e;
}
</style>
