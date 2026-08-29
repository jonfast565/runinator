<template>
  <div class="grid gap-4">
    <label class="flex items-center gap-2 text-sm">
      <input v-model="enabled" type="checkbox" />
      <span>Manage correlated executions for this pipeline</span>
    </label>

    <section
      v-if="!enabled"
      class="grid gap-4 rounded border border-border bg-surface-subtle p-4 text-sm"
    >
      <div class="flex items-center gap-1">
        <h3 class="m-0 text-base font-semibold text-fg">Set up correlated execution</h3>
        <HelpBubble
          text="Connect external events to this pipeline by matching provider events to a correlation scope and lifecycle. Define whether updates observe, pause, restart, or signal a run, then map the result, evidence, and workspace state retained for each phase."
          label="About correlated execution"
        />
      </div>
      <div>
        <button type="button" class="btn btn-primary" @click="enableConfiguration">
          Enable orchestration editor
        </button>
      </div>
    </section>

    <template v-else>
      <nav class="flex flex-wrap gap-1 border-b border-border">
        <button
          v-for="item in tabs"
          :key="item"
          type="button"
          class="px-3 py-2 text-sm"
          :class="tab === item ? 'border-b-2 border-accent text-fg' : 'text-fg-muted'"
          @click="tab = item"
        >
          {{ item }}
        </button>
      </nav>

      <div v-if="tab === 'Admission Routes'" class="grid gap-3">
        <label class="grid gap-1 text-sm"><span>Correlation scope</span><input v-model="scope" required /></label>
        <section
          v-if="routes.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No admission routes yet</strong>
          Add a provider event that should start, record, or control a correlated pipeline run.
        </section>
        <article v-for="(route, routeIndex) in routes" :key="route.id" class="grid gap-2 rounded border border-border p-3">
          <div class="grid gap-2 md:grid-cols-5">
            <label class="grid gap-1 text-xs"><span>Event</span><input v-model="route.event_type" list="orchestration-events" /></label>
            <label class="grid gap-1 text-xs"><span>Lifecycle</span><select v-model="route.lifecycle" @change="normalizeRoute(route)"><option value="unbound">Unbound</option><option value="active">Active</option><option value="terminal">Terminal</option></select></label>
            <label class="grid gap-1 text-xs"><span>Action</span><select v-model="route.action" @change="normalizeRoute(route)"><option v-for="action in actionsFor(route.lifecycle)" :key="action" :value="action">{{ action }}</option></select></label>
            <label class="grid gap-1 text-xs"><span>Intent</span><select v-model="route.intent" :disabled="route.action !== 'dispatch'"><option value="">Select intent</option><option v-for="intent in intents" :key="intent.id" :value="intent.name">{{ intent.name }}</option></select></label>
            <button type="button" class="btn btn-danger self-end" @click="routes.splice(routeIndex, 1)">Remove</button>
          </div>
          <div v-for="(predicate, predicateIndex) in route.predicates" :key="predicate.id" class="grid gap-2 md:grid-cols-[1fr_10rem_1fr_auto]">
            <input v-model="predicate.pointer" list="orchestration-pointers" placeholder="/payload/path" />
            <select v-model="predicate.operator"><option value="equal">equals</option><option value="not_equal">not equal</option><option value="in">in</option><option value="contains">contains</option><option value="exists">exists</option></select>
            <input v-model="predicate.valueText" :disabled="predicate.operator === 'exists'" placeholder='JSON value, e.g. "ready"' />
            <button type="button" class="btn btn-sm" @click="route.predicates.splice(predicateIndex, 1)">×</button>
          </div>
          <button type="button" class="btn btn-sm w-fit" @click="addPredicate(route)">Add predicate</button>
        </article>
        <button type="button" class="btn w-fit" @click="addRoute">Add route</button>
      </div>

      <div v-else-if="tab === 'Intents'" class="grid gap-3">
        <section
          v-if="intents.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No intents yet</strong>
          Add an intent before routing active events to dispatch, pause, restart, or signal a run.
        </section>
        <article v-for="(intent, index) in intents" :key="intent.id" class="grid gap-2 rounded border border-border p-3 md:grid-cols-4">
          <label class="grid gap-1 text-xs"><span>Name</span><input v-model="intent.name" /></label>
          <label class="grid gap-1 text-xs"><span>Effect</span><select v-model="intent.effect"><option v-for="effect in effects" :key="effect" :value="effect">{{ effect }}</option></select></label>
          <label class="grid gap-1 text-xs"><span>Unique priority</span><input v-model.number="intent.priority" type="number" /></label>
          <label class="grid gap-1 text-xs"><span>Coalesce seconds</span><input v-model.number="intent.coalesce_seconds" min="0" type="number" /></label>
          <label class="grid gap-1 text-xs"><span>Stop epoch</span><select v-model="intent.stop"><option value="cancel">cancel</option><option value="pause">pause</option><option value="none">none</option></select></label>
          <label class="grid gap-1 text-xs"><span>Restart</span><select v-model="intent.restart_kind"><option value="entry">entry</option><option value="current">current</option><option value="member">member</option></select></label>
          <label class="grid gap-1 text-xs"><span>Restart member</span><select v-model="intent.restart_member" :disabled="intent.restart_kind !== 'member'"><option value="">Select member</option><option v-for="member in members" :key="member" :value="member">{{ member }}</option></select></label>
          <label class="grid gap-1 text-xs"><span>Subject revision pointer</span><input v-model="intent.subject_revision_pointer" list="orchestration-pointers" placeholder="/subject_revision" /></label>
          <label class="grid gap-1 text-xs"><span>Workflow signal (defaults to intent)</span><input v-model="intent.signal_name" :disabled="intent.effect !== 'signal'" placeholder="external_update" /></label>
          <label class="flex items-end gap-2 text-xs"><input v-model="intent.allow_self_originated" type="checkbox" />Allow self-originated</label>
          <button type="button" class="btn btn-danger w-fit" @click="intents.splice(index, 1)">Remove</button>
        </article>
        <button type="button" class="btn w-fit" @click="addIntent">Add intent</button>
      </div>

      <div v-else-if="tab === 'Budgets'" class="grid gap-3">
        <section
          v-if="budgets.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No retry budgets yet</strong>
          Add a budget only when a failure class needs bounded retries or a recovery handoff.
        </section>
        <article v-for="(budget, index) in budgets" :key="budget.id" class="grid gap-2 rounded border border-border p-3 md:grid-cols-[1fr_10rem_12rem_1fr_auto]">
          <input v-model="budget.name" placeholder="failure class" />
          <input v-model.number="budget.attempts" type="number" min="1" />
          <select v-model="budget.exhausted"><option value="fail">fail</option><option value="pause">pause</option><option value="terminate">terminate</option></select>
          <select v-model="budget.handoff"><option value="">No handoff</option><option v-for="member in members" :key="member" :value="member">handoff to {{ member }}</option></select>
          <button type="button" class="btn btn-danger" @click="budgets.splice(index, 1)">Remove</button>
        </article>
        <button type="button" class="btn w-fit" @click="addBudget">Add budget</button>
      </div>

      <div v-else-if="tab === 'Phase Mappings'" class="grid gap-3">
        <section
          v-if="phases.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          Add a workflow to this pipeline before configuring phase result mappings.
        </section>
        <article v-for="phase in phases" :key="phase.member" class="grid gap-2 rounded border border-border p-3 md:grid-cols-2">
          <strong class="md:col-span-2">{{ phase.member }}</strong>
          <label v-for="pointer in resultPointers" :key="pointer.key" class="grid gap-1 text-xs"><span>{{ pointer.label }}</span><input v-model="phase[pointer.key]" list="orchestration-pointers" placeholder="/result/path" /></label>
        </article>
      </div>

      <div v-else-if="tab === 'Workspaces'" class="grid gap-3">
        <section
          v-if="phases.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          Add a workflow to this pipeline before configuring phase workspace leases.
        </section>
        <article v-for="phase in phases" :key="phase.member" class="grid gap-2 rounded border border-border p-3 md:grid-cols-3">
          <label class="flex items-center gap-2 text-sm md:col-span-3"><input v-model="phase.workspace_enabled" type="checkbox" /><strong>{{ phase.member }}</strong></label>
          <template v-if="phase.workspace_enabled">
            <label class="grid gap-1 text-xs"><span>Opaque scope</span><input v-model="phase.workspace_scope" /></label>
            <label class="grid gap-1 text-xs"><span>Lease seconds</span><input v-model.number="phase.lease_seconds" type="number" min="1" /></label>
            <label class="grid gap-1 text-xs"><span>Recovery</span><select v-model="phase.recovery"><option value="replace">replace</option><option value="wait">wait</option><option value="fail">fail</option></select></label>
            <label class="flex items-center gap-2 text-xs"><input v-model="phase.reuse" type="checkbox" />Reuse compatible workspace</label>
            <label class="grid gap-1 text-xs md:col-span-2"><span>Worker requirements JSON</span><input v-model="phase.requirementsText" /></label>
          </template>
        </article>
      </div>

      <pre v-else class="max-h-[32rem] overflow-auto rounded bg-surface-raised p-3 text-xs">{{ sourcePreview }}</pre>
      <datalist id="orchestration-events"><option v-for="event in canonicalEvents" :key="event" :value="event" /></datalist>
      <datalist id="orchestration-pointers"><option v-for="pointer in canonicalPointers" :key="pointer" :value="pointer" /></datalist>
      <p v-if="errors.length" class="error m-0 whitespace-pre-line text-sm">{{ errors.join("\n") }}</p>
    </template>

    <div class="flex justify-end gap-2">
      <button type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button type="button" class="btn btn-primary" :disabled="errors.length > 0" @click="save">Save pipeline revision</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import HelpBubble from "../shared/HelpBubble.vue";
import type {
  AdapterKindMetadata,
  IngressAction,
  IngressLifecycle,
  IngressPolicy,
  IngressPredicateOperator,
  JsonRecord,
  JsonValue,
  OrchestrationPolicy,
  Pipeline,
} from "../../../core/domain/models";

const props = defineProps<{ pipeline: Pipeline; adapterKinds: AdapterKindMetadata[] }>();
const emit = defineEmits<{ save: [metadata: JsonRecord]; cancel: [] }>();
const tabs = ["Admission Routes", "Intents", "Budgets", "Phase Mappings", "Workspaces", "Preview"] as const;
type Tab = (typeof tabs)[number];
type Effect = "terminate" | "suspend" | "resume" | "supersede" | "observe" | "signal";
type RestartKind = "entry" | "current" | "member";
type Exhaustion = "fail" | "pause" | "terminate";
type Recovery = "replace" | "wait" | "fail";
interface PredicateDraft { id: string; pointer: string; operator: IngressPredicateOperator; valueText: string }
interface RouteDraft { id: string; event_type: string; lifecycle: IngressLifecycle; action: IngressAction; intent: string; predicates: PredicateDraft[] }
interface IntentDraft { id: string; name: string; effect: Effect; priority: number; coalesce_seconds: number; stop: "pause" | "cancel" | "none"; restart_kind: RestartKind; restart_member: string; subject_revision_pointer: string; signal_name: string; allow_self_originated: boolean }
interface BudgetDraft { id: string; name: string; attempts: number; exhausted: Exhaustion; handoff: string }
interface PhaseDraft { member: string; subject_revision: string; resources: string; evidence: string; failure_class: string; correlations: string; workspace_enabled: boolean; workspace_scope: string; lease_seconds: number; reuse: boolean; recovery: Recovery; requirementsText: string }

const metadata = props.pipeline.metadata;
const existingIngress = metadata.ingress as IngressPolicy | undefined;
const existingPolicy = metadata.orchestration as OrchestrationPolicy | undefined;
const enabled = ref(Boolean(existingPolicy));
const tab = ref<Tab>("Admission Routes");
const scope = ref(existingIngress?.scope ?? "correlations");
const members = props.pipeline.graph.members.map((member) => member.key);
const effects: Effect[] = ["terminate", "suspend", "resume", "supersede", "observe", "signal"];
const resultPointers = [
  { key: "subject_revision", label: "Subject revision" },
  { key: "resources", label: "Resources" },
  { key: "evidence", label: "Evidence" },
  { key: "failure_class", label: "Failure class" },
  { key: "correlations", label: "Correlation aliases" },
] as const;

const routes = reactive<RouteDraft[]>((existingIngress?.routes ?? []).map((route) => ({
  id: crypto.randomUUID(), event_type: route.event_type, lifecycle: route.lifecycle,
  action: route.action, intent: route.intent ?? "", predicates: route.predicates.map((predicate) => ({
    id: crypto.randomUUID(), pointer: predicate.pointer, operator: predicate.operator,
    valueText: predicate.value === undefined ? "null" : JSON.stringify(predicate.value),
  })),
})));
const intents = reactive<IntentDraft[]>(Object.entries(existingPolicy?.intents ?? {}).map(([name, intent]) => ({
  id: crypto.randomUUID(), name, effect: intent.effect, priority: intent.priority,
  coalesce_seconds: intent.coalesce_seconds ?? 0, stop: intent.stop ?? "cancel",
  restart_kind: intent.restart?.kind ?? "entry", restart_member: intent.restart?.member ?? "",
  subject_revision_pointer: intent.subject_revision_pointer ?? "", signal_name: intent.signal_name ?? "",
  allow_self_originated: intent.allow_self_originated ?? false,
})));
const budgets = reactive<BudgetDraft[]>(Object.entries(existingPolicy?.budgets ?? {}).map(([name, budget]) => ({ id: crypto.randomUUID(), name, attempts: budget.attempts, exhausted: budget.exhausted, handoff: budget.handoff ?? "" })));
const phases = reactive<PhaseDraft[]>(members.map((member) => {
  const phase = existingPolicy?.phases[member];
  return { member, subject_revision: phase?.result.subject_revision ?? "", resources: phase?.result.resources ?? "", evidence: phase?.result.evidence ?? "", failure_class: phase?.result.failure_class ?? "", correlations: phase?.result.correlations ?? "", workspace_enabled: Boolean(phase?.workspace), workspace_scope: phase?.workspace?.scope ?? "", lease_seconds: phase?.workspace?.lease_seconds ?? 300, reuse: phase?.workspace?.reuse ?? false, recovery: phase?.workspace?.recovery ?? "replace", requirementsText: JSON.stringify(phase?.workspace?.requirements ?? {}, null, 0) };
}));

const canonicalEvents = computed(() => [...new Set(props.adapterKinds.flatMap((kind) => kind.event_names))].sort());
const canonicalPointers = computed(() => [...new Set(props.adapterKinds.flatMap((kind) => kind.canonical_pointers))].sort());
const errors = computed(() => validate());
const sourcePreview = computed(() => renderSource());

function actionsFor(lifecycle: IngressLifecycle): IngressAction[] { return lifecycle === "unbound" ? ["start", "record"] : lifecycle === "terminal" ? ["requeue", "record"] : ["dispatch", "interrupt", "queue", "record"]; }

function normalizeRoute(route: RouteDraft): void { if (!actionsFor(route.lifecycle).includes(route.action)) {route.action = actionsFor(route.lifecycle)[0];} if (route.action !== "dispatch") {route.intent = "";} }

function addRoute(): void { routes.push({ id: crypto.randomUUID(), event_type: canonicalEvents.value[0] ?? "updated", lifecycle: "active", action: "dispatch", intent: intents.length > 0 ? intents[0].name : "", predicates: [] }); }

function addPredicate(route: RouteDraft): void { route.predicates.push({ id: crypto.randomUUID(), pointer: canonicalPointers.value[0] ?? "/", operator: "equal", valueText: "null" }); }

function addIntent(): void { intents.push({ id: crypto.randomUUID(), name: `intent_${String(intents.length + 1)}`, effect: "observe", priority: 10 - intents.length, coalesce_seconds: 0, stop: "cancel", restart_kind: "entry", restart_member: "", subject_revision_pointer: "", signal_name: "", allow_self_originated: false }); }

function addBudget(): void { budgets.push({ id: crypto.randomUUID(), name: `failure_${String(budgets.length + 1)}`, attempts: 1, exhausted: "pause", handoff: "" }); }

function enableConfiguration(): void {
  enabled.value = true;
  tab.value = "Admission Routes";
}

function parseJson(text: string): JsonValue { return JSON.parse(text) as JsonValue; }

function pointerValid(pointer: string): boolean { return pointer === "" || pointer.startsWith("/"); }

function validate(): string[] {
  if (!enabled.value) {return [];}
  const issues: string[] = [];
  if (!scope.value.trim()) {issues.push("Admission scope is required.");}
  const priorities = new Set<number>();
  const names = new Set(intents.map((intent) => intent.name));
  for (const intent of intents) { if (!intent.name.trim()) {issues.push("Intent names are required.");} if (priorities.has(intent.priority)) {issues.push(`Intent priority ${String(intent.priority)} is duplicated.`);} priorities.add(intent.priority); if (intent.restart_kind === "member" && !members.includes(intent.restart_member)) {issues.push(`Intent ${intent.name} has an unknown restart member.`);} if (intent.subject_revision_pointer && !pointerValid(intent.subject_revision_pointer)) {issues.push(`Intent ${intent.name} has an invalid subject revision pointer.`);} }
  for (const route of routes) { if (!route.event_type.trim()) {issues.push("Every route needs an event name.");} if (route.action === "dispatch" && !names.has(route.intent)) {issues.push(`Route ${route.event_type} needs a known intent.`);} for (const predicate of route.predicates) { if (!pointerValid(predicate.pointer)) {issues.push(`Predicate pointer ${predicate.pointer} is invalid.`);} if (predicate.operator !== "exists") { try { parseJson(predicate.valueText); } catch { issues.push(`Predicate ${predicate.pointer} has invalid JSON.`); } } } }
  for (const budget of budgets) { if (!budget.name.trim()) {issues.push("Budget names are required.");} if (!Number.isInteger(budget.attempts) || budget.attempts < 1) {issues.push(`Budget ${budget.name || "(unnamed)"} needs at least one attempt.`);} if (budget.handoff && !members.includes(budget.handoff)) {issues.push(`Budget ${budget.name} has an unknown handoff member.`);} }
  for (const phase of phases) { for (const pointer of resultPointers) { if (phase[pointer.key] && !pointerValid(phase[pointer.key])) {issues.push(`${phase.member} ${pointer.label} pointer is invalid.`);} } if (phase.workspace_enabled) { if (!phase.workspace_scope.trim()) {issues.push(`${phase.member} workspace scope is required.`);} try { parseJson(phase.requirementsText); } catch { issues.push(`${phase.member} workspace requirements are invalid JSON.`); } } }
  return [...new Set(issues)];
}

function buildIngress(): IngressPolicy { return { scope: scope.value.trim(), routes: routes.map((route) => ({ event_type: route.event_type.trim(), lifecycle: route.lifecycle, action: route.action, predicates: route.predicates.map((predicate) => ({ pointer: predicate.pointer, operator: predicate.operator, ...(predicate.operator === "exists" ? {} : { value: parseJson(predicate.valueText) }) })), ...(route.action === "dispatch" ? { intent: route.intent } : {}) })) }; }

function buildPolicy(): OrchestrationPolicy { return { intents: Object.fromEntries(intents.map((intent) => [intent.name, { effect: intent.effect, priority: intent.priority, ...(intent.coalesce_seconds > 0 ? { coalesce_seconds: intent.coalesce_seconds } : {}), stop: intent.stop, restart: { kind: intent.restart_kind, ...(intent.restart_kind === "member" ? { member: intent.restart_member } : {}) }, ...(intent.subject_revision_pointer ? { subject_revision_pointer: intent.subject_revision_pointer } : {}), ...(intent.effect === "signal" && intent.signal_name ? { signal_name: intent.signal_name } : {}), allow_self_originated: intent.allow_self_originated }])), budgets: Object.fromEntries(budgets.map((budget) => [budget.name, { attempts: budget.attempts, exhausted: budget.exhausted, ...(budget.handoff ? { handoff: budget.handoff } : {}) }])), phases: Object.fromEntries(phases.map((phase) => [phase.member, { result: { ...(phase.subject_revision ? { subject_revision: phase.subject_revision } : {}), ...(phase.resources ? { resources: phase.resources } : {}), ...(phase.evidence ? { evidence: phase.evidence } : {}), ...(phase.failure_class ? { failure_class: phase.failure_class } : {}), ...(phase.correlations ? { correlations: phase.correlations } : {}) }, ...(phase.workspace_enabled ? { workspace: { scope: phase.workspace_scope, requirements: parseJson(phase.requirementsText), lease_seconds: phase.lease_seconds, reuse: phase.reuse, recovery: phase.recovery } } : {}) }])), defaults: existingPolicy?.defaults ?? null }; }

function save(): void { const next: JsonRecord = { ...metadata }; if (enabled.value) { next.ingress = buildIngress(); next.orchestration = buildPolicy(); } else { delete next.ingress; delete next.orchestration; } emit("save", next); }

function quote(value: string): string { return JSON.stringify(value); }

function renderSource(): string { if (!enabled.value) {return "# orchestration disabled";} const lines = [`ingress scope ${quote(scope.value)} {`]; for (const route of routes) { lines.push(`  on ${quote(route.event_type)} when ${route.lifecycle}`); for (const predicate of route.predicates) {lines.push(`    if ${quote(predicate.pointer)} ${predicate.operator} ${predicate.operator === "exists" ? "" : predicate.valueText}`.trimEnd());} lines.push(`    -> ${route.action}${route.action === "dispatch" ? ` ${quote(route.intent)}` : ""}`, ""); } lines.push("}", "", "orchestration {"); for (const intent of intents) { let rendered = `  intent ${quote(intent.name)} effect ${intent.effect} priority ${String(intent.priority)}`; if (intent.coalesce_seconds > 0) {rendered += ` coalesce ${String(intent.coalesce_seconds)}s`;} if (intent.stop !== "cancel") {rendered += ` stop ${intent.stop}`;} if (intent.restart_kind === "current") {rendered += " restart current";} else if (intent.restart_kind === "member") {rendered += ` restart ${quote(intent.restart_member)}`;} if (intent.subject_revision_pointer) {rendered += ` revision ${quote(intent.subject_revision_pointer)}`;} if (intent.effect === "signal" && intent.signal_name) {rendered += ` signal ${quote(intent.signal_name)}`;} if (intent.allow_self_originated) {rendered += " allow_self_originated";} lines.push(rendered); } for (const budget of budgets) {lines.push(`  budget ${quote(budget.name)} attempts ${String(budget.attempts)} exhausted ${budget.exhausted}${budget.handoff ? ` via ${quote(budget.handoff)}` : ""}`);} for (const phase of phases) { lines.push("", `  phase ${quote(phase.member)} {`); for (const pointer of resultPointers) {if (phase[pointer.key]) {lines.push(`    ${pointer.key} from ${quote(phase[pointer.key])}`);}} if (phase.workspace_enabled) { let workspace = `    workspace scope ${quote(phase.workspace_scope)}`; if (phase.reuse) {workspace += " reuse";} if (phase.lease_seconds !== 300) {workspace += ` lease ${String(phase.lease_seconds)}s`;} if (phase.recovery !== "replace") {workspace += ` recovery ${phase.recovery}`;} if (phase.requirementsText.trim() && phase.requirementsText.trim() !== "{}") {workspace += ` labels ${phase.requirementsText}`;} lines.push(workspace); } lines.push("  }"); } lines.push("}"); return lines.join("\n"); }
</script>
