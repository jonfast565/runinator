<template>
  <div class="grid gap-4">
    <section class="orchestration-status" :class="enabled ? 'is-enabled' : ''">
      <div class="flex min-w-0 items-start gap-3">
        <span class="orchestration-status-icon">
          <Icon :name="enabled ? 'check' : 'branch'" :size="17" />
        </span>
        <div class="min-w-0">
          <p class="orchestration-eyebrow">
            {{ enabled ? "Orchestration enabled" : "Orchestration disabled" }}
          </p>
          <h3>Manage correlated executions</h3>
          <p>
            {{
              enabled
                ? `${routes.length} admission routes · ${intents.length} intents · ${budgets.length} retry budgets`
                : "Provider events will not create or control correlated runs for this pipeline."
            }}
          </p>
        </div>
      </div>
      <button
        v-if="enabled"
        type="button"
        class="btn btn-danger btn-sm"
        @click="disableConfirmOpen = true"
      >
        Disable…
      </button>
    </section>

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
          Enable orchestration
        </button>
      </div>
    </section>

    <template v-else>
      <section v-if="issues.length" class="orchestration-errors" aria-live="polite">
        <div>
          <strong>{{ issues.length }} issue{{ issues.length === 1 ? "" : "s" }} to fix</strong>
          <p>Open a highlighted section to review its fields.</p>
        </div>
        <div class="flex flex-wrap gap-1.5">
          <button
            v-for="item in issueTabs"
            :key="item.tab"
            type="button"
            class="btn btn-sm"
            @click="tab = item.tab"
          >
            {{ item.tab }} · {{ item.count }}
          </button>
        </div>
      </section>

      <nav class="orchestration-tabs" aria-label="Orchestration settings" role="tablist">
        <button
          v-for="item in tabs"
          :key="item"
          type="button"
          role="tab"
          :aria-selected="tab === item"
          :class="{ 'is-active': tab === item, 'has-errors': tabIssueCount(item) > 0 }"
          @click="tab = item"
        >
          <span>{{ item }}</span>
          <span v-if="tabIssueCount(item)" class="orchestration-tab-count">
            {{ tabIssueCount(item) }}
          </span>
        </button>
      </nav>

      <div v-if="tab === 'Admission Routes'" class="grid gap-3">
        <header class="orchestration-section-heading">
          <div>
            <h3>Choose which events enter this pipeline</h3>
            <p>Routes are checked in order against an orchestration's current lifecycle.</p>
          </div>
          <button type="button" class="btn btn-primary btn-sm" @click="addRoute">
            <Icon name="plus" :size="15" />
            Add route
          </button>
        </header>
        <label class="grid gap-1 text-sm"
          ><span>Correlation scope</span><input v-model="scope" required />
          <small class="text-fg-muted"
            >Events with the same scope and correlation key share an orchestration.</small
          ></label
        >
        <section
          v-if="routes.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No admission routes yet</strong>
          Add a provider event that should start, record, or control a correlated pipeline run.
        </section>
        <article v-for="(route, routeIndex) in routes" :key="route.id" class="orchestration-card">
          <div class="orchestration-card-heading">
            <div>
              <p class="orchestration-eyebrow">Route {{ routeIndex + 1 }}</p>
              <strong>{{ route.event_type || "Unnamed provider event" }}</strong>
            </div>
            <button
              type="button"
              class="btn btn-ghost btn-sm text-danger-fg"
              @click="routes.splice(routeIndex, 1)"
            >
              Remove route
            </button>
          </div>
          <div class="grid gap-2 md:grid-cols-5">
            <label class="grid gap-1 text-xs"
              ><span>Event</span
              ><input v-model="route.event_type" list="orchestration-events" required
            /></label>
            <label class="grid gap-1 text-xs"
              ><span>Lifecycle</span
              ><select v-model="route.lifecycle" @change="normalizeRoute(route)">
                <option value="unbound">Unbound</option>
                <option value="active">Active</option>
                <option value="terminal">Terminal</option>
              </select></label
            >
            <label class="grid gap-1 text-xs"
              ><span>Action</span
              ><select v-model="route.action" @change="normalizeRoute(route)">
                <option v-for="action in actionsFor(route.lifecycle)" :key="action" :value="action">
                  {{ action }}
                </option>
              </select></label
            >
            <label class="grid gap-1 text-xs"
              ><span>Intent</span
              ><select v-model="route.intent" :disabled="route.action !== 'dispatch'">
                <option value="">Select intent</option>
                <option v-for="intent in intents" :key="intent.id" :value="intent.name">
                  {{ intent.name }}
                </option>
              </select></label
            >
            <div class="self-end text-xs text-fg-muted">
              {{ routeActionHint(route) }}
            </div>
          </div>
          <div
            v-for="(predicate, predicateIndex) in route.predicates"
            :key="predicate.id"
            class="grid gap-2 md:grid-cols-[1fr_10rem_1fr_auto]"
          >
            <input
              v-model="predicate.pointer"
              list="orchestration-pointers"
              placeholder="/payload/path"
            />
            <select v-model="predicate.operator">
              <option value="equal">equals</option>
              <option value="not_equal">not equal</option>
              <option value="in">in</option>
              <option value="contains">contains</option>
              <option value="exists">exists</option>
            </select>
            <input
              v-model="predicate.valueText"
              :disabled="predicate.operator === 'exists'"
              placeholder='JSON value, e.g. "ready"'
            />
            <button
              type="button"
              class="btn btn-sm"
              @click="route.predicates.splice(predicateIndex, 1)"
            >
              ×
            </button>
          </div>
          <button type="button" class="btn btn-sm w-fit" @click="addPredicate(route)">
            Add condition
          </button>
        </article>
      </div>

      <div v-else-if="tab === 'Intents'" class="grid gap-3">
        <header class="orchestration-section-heading">
          <div>
            <h3>Define how active runs react</h3>
            <p>Each intent needs a unique name and priority so event races resolve predictably.</p>
          </div>
          <button type="button" class="btn btn-primary btn-sm" @click="addIntent">
            <Icon name="plus" :size="15" />
            Add intent
          </button>
        </header>
        <section
          v-if="intents.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No intents yet</strong>
          Add an intent before routing active events to dispatch, pause, restart, or signal a run.
        </section>
        <article
          v-for="(intent, index) in intents"
          :key="intent.id"
          class="orchestration-card grid gap-2 md:grid-cols-4"
        >
          <div class="orchestration-card-heading md:col-span-4">
            <div>
              <p class="orchestration-eyebrow">Intent {{ index + 1 }}</p>
              <strong>{{ intent.name || "Unnamed intent" }}</strong>
            </div>
            <button
              type="button"
              class="btn btn-ghost btn-sm text-danger-fg"
              :disabled="intentReferenceCount(intent.name) > 0"
              :title="intentRemovalHint(intent.name)"
              @click="removeIntent(index)"
            >
              Remove intent
            </button>
          </div>
          <label class="grid gap-1 text-xs"><span>Name</span><input v-model="intent.name" /></label>
          <label class="grid gap-1 text-xs"
            ><span>Effect</span
            ><select v-model="intent.effect">
              <option v-for="effect in effects" :key="effect" :value="effect">{{ effect }}</option>
            </select></label
          >
          <label class="grid gap-1 text-xs"
            ><span>Unique priority</span><input v-model.number="intent.priority" type="number"
          /></label>
          <label class="grid gap-1 text-xs"
            ><span>Coalesce seconds</span
            ><input v-model.number="intent.coalesce_seconds" min="0" type="number"
          /></label>
          <label class="grid gap-1 text-xs"
            ><span>Stop epoch</span
            ><select v-model="intent.stop">
              <option value="cancel">cancel</option>
              <option value="pause">pause</option>
              <option value="none">none</option>
            </select></label
          >
          <label class="grid gap-1 text-xs"
            ><span>Restart</span
            ><select v-model="intent.restart_kind">
              <option value="entry">entry</option>
              <option value="current">current</option>
              <option value="member">member</option>
            </select></label
          >
          <label class="grid gap-1 text-xs"
            ><span>Restart member</span
            ><select v-model="intent.restart_member" :disabled="intent.restart_kind !== 'member'">
              <option value="">Select member</option>
              <option v-for="member in members" :key="member" :value="member">{{ member }}</option>
            </select></label
          >
          <label class="grid gap-1 text-xs"
            ><span>Subject revision pointer</span
            ><input
              v-model="intent.subject_revision_pointer"
              list="orchestration-pointers"
              placeholder="/subject_revision"
          /></label>
          <label class="grid gap-1 text-xs"
            ><span>Workflow signal (defaults to intent)</span
            ><input
              v-model="intent.signal_name"
              :disabled="intent.effect !== 'signal'"
              placeholder="external_update"
          /></label>
          <label class="flex items-end gap-2 text-xs"
            ><input v-model="intent.allow_self_originated" type="checkbox" />Allow
            self-originated</label
          >
          <p v-if="intentReferenceCount(intent.name)" class="m-0 self-end text-xs text-fg-muted">
            Used by {{ intentReferenceCount(intent.name) }} admission route{{
              intentReferenceCount(intent.name) === 1 ? "" : "s"
            }}.
          </p>
        </article>
      </div>

      <div v-else-if="tab === 'Budgets'" class="grid gap-3">
        <header class="orchestration-section-heading">
          <div>
            <h3>Bound retries by failure class</h3>
            <p>Budget names must be unique or one policy would overwrite another.</p>
          </div>
          <button type="button" class="btn btn-primary btn-sm" @click="addBudget">
            <Icon name="plus" :size="15" />
            Add budget
          </button>
        </header>
        <section
          v-if="budgets.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          <strong class="block text-fg">No retry budgets yet</strong>
          Add a budget only when a failure class needs bounded retries or a recovery handoff.
        </section>
        <article v-for="(budget, index) in budgets" :key="budget.id" class="orchestration-card">
          <div class="orchestration-card-heading">
            <div>
              <p class="orchestration-eyebrow">Retry budget {{ index + 1 }}</p>
              <strong>{{ budget.name || "Unnamed failure class" }}</strong>
            </div>
            <button
              type="button"
              class="btn btn-ghost btn-sm text-danger-fg"
              @click="budgets.splice(index, 1)"
            >
              Remove budget
            </button>
          </div>
          <div class="grid gap-2 md:grid-cols-[1fr_10rem_12rem_1fr]">
            <label class="grid gap-1 text-xs"
              ><span>Failure class</span><input v-model="budget.name" required
            /></label>
            <label class="grid gap-1 text-xs"
              ><span>Maximum attempts</span
              ><input v-model.number="budget.attempts" type="number" min="1" step="1"
            /></label>
            <label class="grid gap-1 text-xs"
              ><span>When exhausted</span
              ><select v-model="budget.exhausted">
                <option value="fail">Fail orchestration</option>
                <option value="pause">Pause for review</option>
                <option value="terminate">Terminate</option>
              </select></label
            >
            <label class="grid gap-1 text-xs"
              ><span>Recovery handoff</span
              ><select v-model="budget.handoff">
                <option value="">No handoff</option>
                <option v-for="member in members" :key="member" :value="member">
                  Handoff to {{ member }}
                </option>
              </select></label
            >
          </div>
        </article>
      </div>

      <div v-else-if="tab === 'Phase Mappings'" class="grid gap-3">
        <header class="orchestration-section-heading">
          <div>
            <h3>Map workflow results into orchestration state</h3>
            <p>Leave a mapping blank when that phase does not produce the value.</p>
          </div>
        </header>
        <section
          v-if="phases.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          Add a workflow to this pipeline before configuring phase result mappings.
        </section>
        <article
          v-for="phase in phases"
          :key="phase.member"
          class="grid gap-2 rounded border border-border p-3 md:grid-cols-2"
        >
          <strong class="md:col-span-2">{{ phase.member }}</strong>
          <label v-for="pointer in resultPointers" :key="pointer.key" class="grid gap-1 text-xs"
            ><span>{{ pointer.label }}</span
            ><input
              v-model="phase[pointer.key]"
              list="orchestration-pointers"
              placeholder="/result/path"
          /></label>
        </article>
      </div>

      <div v-else-if="tab === 'Workspaces'" class="grid gap-3">
        <header class="orchestration-section-heading">
          <div>
            <h3>Lease worker-local workspaces</h3>
            <p>Only enable a lease for phases that need durable machine-local state.</p>
          </div>
        </header>
        <section
          v-if="phases.length === 0"
          class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
        >
          Add a workflow to this pipeline before configuring phase workspace leases.
        </section>
        <article
          v-for="phase in phases"
          :key="phase.member"
          class="grid gap-2 rounded border border-border p-3 md:grid-cols-3"
        >
          <label class="flex items-center gap-2 text-sm md:col-span-3"
            ><input v-model="phase.workspace_enabled" type="checkbox" /><strong>{{
              phase.member
            }}</strong></label
          >
          <template v-if="phase.workspace_enabled">
            <label class="grid gap-1 text-xs"
              ><span>Opaque scope</span><input v-model="phase.workspace_scope"
            /></label>
            <label class="grid gap-1 text-xs"
              ><span>Lease seconds</span
              ><input v-model.number="phase.lease_seconds" type="number" min="1" step="1"
            /></label>
            <label class="grid gap-1 text-xs"
              ><span>Recovery</span
              ><select v-model="phase.recovery">
                <option value="replace">replace</option>
                <option value="wait">wait</option>
                <option value="fail">fail</option>
              </select></label
            >
            <label class="flex items-center gap-2 text-xs"
              ><input v-model="phase.reuse" type="checkbox" />Reuse compatible workspace</label
            >
            <label class="grid gap-1 text-xs md:col-span-2"
              ><span>Worker requirements JSON</span><input v-model="phase.requirementsText"
            /></label>
          </template>
        </article>
      </div>

      <pre v-else class="max-h-[32rem] overflow-auto rounded bg-surface-raised p-3 text-xs">{{
        sourcePreview
      }}</pre>
      <datalist id="orchestration-events">
        <option v-for="event in canonicalEvents" :key="event" :value="event" />
      </datalist>
      <datalist id="orchestration-pointers">
        <option v-for="pointer in canonicalPointers" :key="pointer" :value="pointer" />
      </datalist>
      <section v-if="activeTabIssues.length" class="rounded border border-danger bg-danger-bg p-3">
        <strong class="text-sm text-danger-fg">Fix before saving</strong>
        <ul class="mt-2 grid gap-1 pl-5 text-sm text-danger-fg">
          <li v-for="issue in activeTabIssues" :key="issue.message">{{ issue.message }}</li>
        </ul>
      </section>
    </template>

    <div class="orchestration-actions">
      <p v-if="enabled && issues.length" class="m-0 text-xs text-danger-fg">
        Review {{ issues.length }} issue{{ issues.length === 1 ? "" : "s" }} before saving.
      </p>
      <span v-else class="flex-1" />
      <button type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button
        type="button"
        class="btn btn-primary"
        :disabled="issues.length > 0"
        :title="issues.length ? 'Resolve the highlighted issues before saving.' : ''"
        @click="save"
      >
        Save pipeline revision
      </button>
    </div>

    <section v-if="disableConfirmOpen" class="orchestration-disable-confirm" role="alert">
      <div>
        <strong>Remove orchestration from this pipeline?</strong>
        <p>
          Saving after this will delete all admission routes, intents, budgets, phase mappings, and
          workspace policies from the next pipeline revision.
        </p>
      </div>
      <div class="flex flex-wrap justify-end gap-2">
        <button type="button" class="btn" @click="disableConfirmOpen = false">Keep enabled</button>
        <button type="button" class="btn btn-danger" @click="confirmDisable">
          Disable and remove configuration
        </button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { createUuid } from "../../../core/utils/uuid";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
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
const tabs = [
  "Admission Routes",
  "Intents",
  "Budgets",
  "Phase Mappings",
  "Workspaces",
  "Preview",
] as const;
type Tab = (typeof tabs)[number];
type Effect = "terminate" | "suspend" | "resume" | "supersede" | "observe" | "signal";
type RestartKind = "entry" | "current" | "member";
type Exhaustion = "fail" | "pause" | "terminate";
type Recovery = "replace" | "wait" | "fail";
interface ValidationIssue {
  tab: Tab;
  message: string;
}
interface PredicateDraft {
  id: string;
  pointer: string;
  operator: IngressPredicateOperator;
  valueText: string;
}
interface RouteDraft {
  id: string;
  event_type: string;
  lifecycle: IngressLifecycle;
  action: IngressAction;
  intent: string;
  predicates: PredicateDraft[];
}
interface IntentDraft {
  id: string;
  name: string;
  effect: Effect;
  priority: number;
  coalesce_seconds: number;
  stop: "pause" | "cancel" | "none";
  restart_kind: RestartKind;
  restart_member: string;
  subject_revision_pointer: string;
  signal_name: string;
  allow_self_originated: boolean;
}
interface BudgetDraft {
  id: string;
  name: string;
  attempts: number;
  exhausted: Exhaustion;
  handoff: string;
}
interface PhaseDraft {
  member: string;
  subject_revision: string;
  resources: string;
  evidence: string;
  failure_class: string;
  correlations: string;
  workspace_enabled: boolean;
  workspace_scope: string;
  lease_seconds: number;
  reuse: boolean;
  recovery: Recovery;
  requirementsText: string;
}

const metadata = props.pipeline.metadata;
const existingIngress = metadata.ingress as IngressPolicy | undefined;
const existingPolicy = metadata.orchestration as OrchestrationPolicy | undefined;
const enabled = ref(Boolean(existingPolicy));
const disableConfirmOpen = ref(false);
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

const routes = reactive<RouteDraft[]>(
  (existingIngress?.routes ?? []).map((route) => ({
    id: createUuid(),
    event_type: route.event_type,
    lifecycle: route.lifecycle,
    action: route.action,
    intent: route.intent ?? "",
    predicates: route.predicates.map((predicate) => ({
      id: createUuid(),
      pointer: predicate.pointer,
      operator: predicate.operator,
      valueText: predicate.value === undefined ? "null" : JSON.stringify(predicate.value),
    })),
  })),
);
const intents = reactive<IntentDraft[]>(
  Object.entries(existingPolicy?.intents ?? {}).map(([name, intent]) => ({
    id: createUuid(),
    name,
    effect: intent.effect,
    priority: intent.priority,
    coalesce_seconds: intent.coalesce_seconds ?? 0,
    stop: intent.stop ?? "cancel",
    restart_kind: intent.restart?.kind ?? "entry",
    restart_member: intent.restart?.member ?? "",
    subject_revision_pointer: intent.subject_revision_pointer ?? "",
    signal_name: intent.signal_name ?? "",
    allow_self_originated: intent.allow_self_originated ?? false,
  })),
);
const budgets = reactive<BudgetDraft[]>(
  Object.entries(existingPolicy?.budgets ?? {}).map(([name, budget]) => ({
    id: createUuid(),
    name,
    attempts: budget.attempts,
    exhausted: budget.exhausted,
    handoff: budget.handoff ?? "",
  })),
);
const phases = reactive<PhaseDraft[]>(
  members.map((member) => {
    const phase = existingPolicy?.phases[member];
    return {
      member,
      subject_revision: phase?.result.subject_revision ?? "",
      resources: phase?.result.resources ?? "",
      evidence: phase?.result.evidence ?? "",
      failure_class: phase?.result.failure_class ?? "",
      correlations: phase?.result.correlations ?? "",
      workspace_enabled: Boolean(phase?.workspace),
      workspace_scope: phase?.workspace?.scope ?? "",
      lease_seconds: phase?.workspace?.lease_seconds ?? 300,
      reuse: phase?.workspace?.reuse ?? false,
      recovery: phase?.workspace?.recovery ?? "replace",
      requirementsText: JSON.stringify(phase?.workspace?.requirements ?? {}, null, 0),
    };
  }),
);

const canonicalEvents = computed(() =>
  [...new Set(props.adapterKinds.flatMap((kind) => kind.event_names))].sort(),
);
const canonicalPointers = computed(() =>
  [...new Set(props.adapterKinds.flatMap((kind) => kind.canonical_pointers))].sort(),
);
const issues = computed(() => validate());
const activeTabIssues = computed(() => issues.value.filter((issue) => issue.tab === tab.value));
const issueTabs = computed(() =>
  tabs.map((item) => ({ tab: item, count: tabIssueCount(item) })).filter((item) => item.count > 0),
);
const sourcePreview = computed(() => renderSource());

function actionsFor(lifecycle: IngressLifecycle): IngressAction[] {
  return lifecycle === "unbound"
    ? ["start", "record"]
    : lifecycle === "terminal"
      ? ["requeue", "record"]
      : ["dispatch", "interrupt", "queue", "record"];
}

function normalizeRoute(route: RouteDraft): void {
  if (!actionsFor(route.lifecycle).includes(route.action)) {
    route.action = actionsFor(route.lifecycle)[0];
  }

  if (route.action !== "dispatch") {
    route.intent = "";
  }
}

function addRoute(): void {
  const hasIntent = intents.length > 0;
  routes.push({
    id: createUuid(),
    event_type: canonicalEvents.value[0] ?? "updated",
    lifecycle: hasIntent ? "active" : "unbound",
    action: hasIntent ? "dispatch" : "start",
    intent: hasIntent ? intents[0].name : "",
    predicates: [],
  });
}

function addPredicate(route: RouteDraft): void {
  route.predicates.push({
    id: createUuid(),
    pointer: canonicalPointers.value[0] ?? "/",
    operator: "equal",
    valueText: "null",
  });
}

function addIntent(): void {
  intents.push({
    id: createUuid(),
    name: `intent_${String(intents.length + 1)}`,
    effect: "observe",
    priority: 10 - intents.length,
    coalesce_seconds: 0,
    stop: "cancel",
    restart_kind: "entry",
    restart_member: "",
    subject_revision_pointer: "",
    signal_name: "",
    allow_self_originated: false,
  });
}

function addBudget(): void {
  budgets.push({
    id: createUuid(),
    name: `failure_${String(budgets.length + 1)}`,
    attempts: 1,
    exhausted: "pause",
    handoff: "",
  });
}

function enableConfiguration(): void {
  enabled.value = true;
  tab.value = "Admission Routes";
}

function confirmDisable(): void {
  enabled.value = false;
  disableConfirmOpen.value = false;
}

function tabIssueCount(item: Tab): number {
  return issues.value.filter((issue) => issue.tab === item).length;
}

function intentReferenceCount(name: string): number {
  return routes.filter((route) => route.action === "dispatch" && route.intent === name).length;
}

function intentRemovalHint(name: string): string {
  const count = intentReferenceCount(name);
  return count > 0
    ? `Change the ${String(count)} admission route${count === 1 ? "" : "s"} that use this intent before removing it.`
    : "Remove intent";
}

function removeIntent(index: number): void {
  const intent = intents[index];

  if (intentReferenceCount(intent.name) > 0) {
    return;
  }

  intents.splice(index, 1);
}

function routeActionHint(route: RouteDraft): string {
  if (route.action === "dispatch") {
    return route.intent ? `Dispatches ${route.intent}` : "Select an intent";
  }

  const labels: Record<IngressAction, string> = {
    start: "Starts a new run",
    interrupt: "Interrupts the active run",
    queue: "Queues until the run settles",
    record: "Records without changing the run",
    requeue: "Starts the next generation",
    dispatch: "Dispatches an intent",
  };
  return labels[route.action];
}

function parseJson(text: string): JsonValue {
  return JSON.parse(text) as JsonValue;
}

function pointerValid(pointer: string): boolean {
  return pointer === "" || pointer.startsWith("/");
}

function duplicateNames(values: string[]): Set<string> {
  const seen = new Set<string>();
  const duplicates = new Set<string>();

  for (const value of values.map((item) => item.trim()).filter(Boolean)) {
    if (seen.has(value)) {
      duplicates.add(value);
    }

    seen.add(value);
  }

  return duplicates;
}

function validate(): ValidationIssue[] {
  if (!enabled.value) {
    return [];
  }

  const found: ValidationIssue[] = [];
  const add = (issueTab: Tab, message: string) => found.push({ tab: issueTab, message });

  if (!scope.value.trim()) {
    add("Admission Routes", "Correlation scope is required.");
  }

  const priorities = new Set<number>();
  const names = new Set(intents.map((intent) => intent.name));

  for (const duplicate of duplicateNames(intents.map((intent) => intent.name))) {
    add("Intents", `Intent name “${duplicate}” is duplicated.`);
  }

  for (const intent of intents) {
    if (!intent.name.trim()) {
      add("Intents", "Every intent needs a name.");
    }

    if (!Number.isFinite(intent.priority)) {
      add("Intents", `Intent ${intent.name || "(unnamed)"} needs a numeric priority.`);
    } else if (priorities.has(intent.priority)) {
      add("Intents", `Intent priority ${String(intent.priority)} is duplicated.`);
    }

    priorities.add(intent.priority);

    if (!Number.isFinite(intent.coalesce_seconds) || intent.coalesce_seconds < 0) {
      add("Intents", `Intent ${intent.name || "(unnamed)"} needs a non-negative coalesce window.`);
    }

    if (intent.restart_kind === "member" && !members.includes(intent.restart_member)) {
      add("Intents", `Intent ${intent.name} has an unknown restart member.`);
    }

    if (intent.subject_revision_pointer && !pointerValid(intent.subject_revision_pointer)) {
      add("Intents", `Intent ${intent.name} has an invalid subject revision pointer.`);
    }
  }

  for (const route of routes) {
    if (!route.event_type.trim()) {
      add("Admission Routes", "Every route needs an event name.");
    }

    if (route.action === "dispatch" && !names.has(route.intent)) {
      add("Admission Routes", `Route ${route.event_type || "(unnamed)"} needs a known intent.`);
    }

    for (const predicate of route.predicates) {
      if (!predicate.pointer || !pointerValid(predicate.pointer)) {
        add(
          "Admission Routes",
          `Route ${route.event_type || "(unnamed)"} has an invalid condition pointer.`,
        );
      }

      if (predicate.operator !== "exists") {
        try {
          parseJson(predicate.valueText);
        } catch {
          add(
            "Admission Routes",
            `Condition ${predicate.pointer || "(unnamed)"} has invalid JSON.`,
          );
        }
      }
    }
  }

  for (const duplicate of duplicateNames(budgets.map((budget) => budget.name))) {
    add("Budgets", `Budget name “${duplicate}” is duplicated.`);
  }

  for (const budget of budgets) {
    if (!budget.name.trim()) {
      add("Budgets", "Every retry budget needs a failure class.");
    }

    if (!Number.isInteger(budget.attempts) || budget.attempts < 1) {
      add(
        "Budgets",
        `Budget ${budget.name || "(unnamed)"} needs at least one whole-number attempt.`,
      );
    }

    if (budget.handoff && !members.includes(budget.handoff)) {
      add("Budgets", `Budget ${budget.name} has an unknown handoff member.`);
    }
  }

  for (const phase of phases) {
    for (const pointer of resultPointers) {
      if (phase[pointer.key] && !pointerValid(phase[pointer.key])) {
        add("Phase Mappings", `${phase.member} ${pointer.label} pointer is invalid.`);
      }
    }

    if (phase.workspace_enabled) {
      if (!phase.workspace_scope.trim()) {
        add("Workspaces", `${phase.member} workspace scope is required.`);
      }

      if (!Number.isInteger(phase.lease_seconds) || phase.lease_seconds < 1) {
        add("Workspaces", `${phase.member} lease must be at least one whole second.`);
      }

      try {
        parseJson(phase.requirementsText);
      } catch {
        add("Workspaces", `${phase.member} workspace requirements are invalid JSON.`);
      }
    }
  }

  return found.filter(
    (issue, index) =>
      found.findIndex((other) => other.tab === issue.tab && other.message === issue.message) ===
      index,
  );
}

function buildIngress(): IngressPolicy {
  return {
    scope: scope.value.trim(),
    routes: routes.map((route) => ({
      event_type: route.event_type.trim(),
      lifecycle: route.lifecycle,
      action: route.action,
      predicates: route.predicates.map((predicate) => ({
        pointer: predicate.pointer,
        operator: predicate.operator,
        ...(predicate.operator === "exists" ? {} : { value: parseJson(predicate.valueText) }),
      })),
      ...(route.action === "dispatch" ? { intent: route.intent } : {}),
    })),
  };
}

function buildPolicy(): OrchestrationPolicy {
  return {
    intents: Object.fromEntries(
      intents.map((intent) => [
        intent.name,
        {
          effect: intent.effect,
          priority: intent.priority,
          ...(intent.coalesce_seconds > 0 ? { coalesce_seconds: intent.coalesce_seconds } : {}),
          stop: intent.stop,
          restart: {
            kind: intent.restart_kind,
            ...(intent.restart_kind === "member" ? { member: intent.restart_member } : {}),
          },
          ...(intent.subject_revision_pointer
            ? { subject_revision_pointer: intent.subject_revision_pointer }
            : {}),
          ...(intent.effect === "signal" && intent.signal_name
            ? { signal_name: intent.signal_name }
            : {}),
          allow_self_originated: intent.allow_self_originated,
        },
      ]),
    ),
    budgets: Object.fromEntries(
      budgets.map((budget) => [
        budget.name,
        {
          attempts: budget.attempts,
          exhausted: budget.exhausted,
          ...(budget.handoff ? { handoff: budget.handoff } : {}),
        },
      ]),
    ),
    phases: Object.fromEntries(
      phases.map((phase) => [
        phase.member,
        {
          result: {
            ...(phase.subject_revision ? { subject_revision: phase.subject_revision } : {}),
            ...(phase.resources ? { resources: phase.resources } : {}),
            ...(phase.evidence ? { evidence: phase.evidence } : {}),
            ...(phase.failure_class ? { failure_class: phase.failure_class } : {}),
            ...(phase.correlations ? { correlations: phase.correlations } : {}),
          },
          ...(phase.workspace_enabled
            ? {
                workspace: {
                  scope: phase.workspace_scope,
                  requirements: parseJson(phase.requirementsText),
                  lease_seconds: phase.lease_seconds,
                  reuse: phase.reuse,
                  recovery: phase.recovery,
                },
              }
            : {}),
        },
      ]),
    ),
    defaults: existingPolicy?.defaults ?? null,
  };
}

function save(): void {
  if (issues.value.length > 0) {
    tab.value = issues.value[0].tab;
    return;
  }

  const next: JsonRecord = { ...metadata };

  if (enabled.value) {
    next.ingress = buildIngress();
    next.orchestration = buildPolicy();
  } else {
    delete next.ingress;
    delete next.orchestration;
  }

  emit("save", next);
}

function quote(value: string): string {
  return JSON.stringify(value);
}

function renderSource(): string {
  if (!enabled.value) {
    return "# orchestration disabled";
  }

  const lines = [`ingress scope ${quote(scope.value)} {`];

  for (const route of routes) {
    lines.push(`  on ${quote(route.event_type)} when ${route.lifecycle}`);

    for (const predicate of route.predicates) {
      lines.push(
        `    if ${quote(predicate.pointer)} ${predicate.operator} ${predicate.operator === "exists" ? "" : predicate.valueText}`.trimEnd(),
      );
    }

    lines.push(
      `    -> ${route.action}${route.action === "dispatch" ? ` ${quote(route.intent)}` : ""}`,
      "",
    );
  }

  lines.push("}", "", "orchestration {");

  for (const intent of intents) {
    let rendered = `  intent ${quote(intent.name)} effect ${intent.effect} priority ${String(intent.priority)}`;

    if (intent.coalesce_seconds > 0) {
      rendered += ` coalesce ${String(intent.coalesce_seconds)}s`;
    }

    if (intent.stop !== "cancel") {
      rendered += ` stop ${intent.stop}`;
    }

    if (intent.restart_kind === "current") {
      rendered += " restart current";
    } else if (intent.restart_kind === "member") {
      rendered += ` restart ${quote(intent.restart_member)}`;
    }

    if (intent.subject_revision_pointer) {
      rendered += ` revision ${quote(intent.subject_revision_pointer)}`;
    }

    if (intent.effect === "signal" && intent.signal_name) {
      rendered += ` signal ${quote(intent.signal_name)}`;
    }

    if (intent.allow_self_originated) {
      rendered += " allow_self_originated";
    }

    lines.push(rendered);
  }

  for (const budget of budgets) {
    lines.push(
      `  budget ${quote(budget.name)} attempts ${String(budget.attempts)} exhausted ${budget.exhausted}${budget.handoff ? ` via ${quote(budget.handoff)}` : ""}`,
    );
  }

  for (const phase of phases) {
    lines.push("", `  phase ${quote(phase.member)} {`);

    for (const pointer of resultPointers) {
      if (phase[pointer.key]) {
        lines.push(`    ${pointer.key} from ${quote(phase[pointer.key])}`);
      }
    }

    if (phase.workspace_enabled) {
      let workspace = `    workspace scope ${quote(phase.workspace_scope)}`;

      if (phase.reuse) {
        workspace += " reuse";
      }

      if (phase.lease_seconds !== 300) {
        workspace += ` lease ${String(phase.lease_seconds)}s`;
      }

      if (phase.recovery !== "replace") {
        workspace += ` recovery ${phase.recovery}`;
      }

      if (phase.requirementsText.trim() && phase.requirementsText.trim() !== "{}") {
        workspace += ` labels ${phase.requirementsText}`;
      }

      lines.push(workspace);
    }

    lines.push("  }");
  }

  lines.push("}");
  return lines.join("\n");
}
</script>

<style scoped>
.orchestration-status,
.orchestration-errors,
.orchestration-disable-confirm {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-4);
}

.orchestration-status.is-enabled {
  border-color: color-mix(in srgb, var(--success-fg) 28%, var(--border));
  background: color-mix(in srgb, var(--success-bg) 48%, var(--surface));
}

.orchestration-status-icon {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--accent-text);
}

.orchestration-status h3,
.orchestration-section-heading h3 {
  margin: 2px 0 0;
  color: var(--text);
  font-size: 14px;
  font-weight: 700;
}

.orchestration-status p:not(.orchestration-eyebrow),
.orchestration-section-heading p,
.orchestration-errors p,
.orchestration-disable-confirm p {
  margin: 4px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.orchestration-eyebrow {
  margin: 0;
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.07em;
  text-transform: uppercase;
}

.orchestration-errors,
.orchestration-disable-confirm {
  border-color: color-mix(in srgb, var(--danger-fg) 32%, var(--border));
  background: var(--danger-bg);
}

.orchestration-errors strong,
.orchestration-disable-confirm strong {
  color: var(--danger-fg);
  font-size: 13px;
}

.orchestration-tabs {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  border-bottom: 1px solid var(--border-subtle);
}

.orchestration-tabs button {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 6px;
  border: 0;
  border-bottom: 2px solid transparent;
  border-radius: 0;
  background: transparent;
  padding: 9px var(--space-3);
  color: var(--text-muted);
  font-size: 13px;
}

.orchestration-tabs button.is-active {
  border-bottom-color: var(--accent);
  color: var(--text);
  font-weight: 700;
}

.orchestration-tabs button.has-errors {
  color: var(--danger-fg);
}

.orchestration-tab-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  border-radius: var(--radius-pill);
  background: var(--danger-bg);
  color: var(--danger-fg);
  font-size: 10px;
  font-weight: 700;
}

.orchestration-section-heading,
.orchestration-card-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.orchestration-card {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.orchestration-card-heading {
  border-bottom: 1px solid var(--border-faint);
  padding-bottom: var(--space-2);
}

.orchestration-card-heading strong {
  color: var(--text);
  font-size: 13px;
}

.orchestration-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  border-top: 1px solid var(--border-subtle);
  padding-top: var(--space-3);
}

.orchestration-actions > p {
  margin-right: auto;
}

@media (max-width: 760px) {
  .orchestration-status,
  .orchestration-errors,
  .orchestration-disable-confirm,
  .orchestration-section-heading {
    flex-direction: column;
  }

  .orchestration-status > button,
  .orchestration-section-heading > button {
    width: 100%;
  }

  .orchestration-actions {
    flex-wrap: wrap;
  }

  .orchestration-actions > p {
    width: 100%;
  }
}
</style>
