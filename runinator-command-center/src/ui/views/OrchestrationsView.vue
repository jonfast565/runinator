<template>
  <section class="pane flex h-full min-h-0 flex-col gap-3 p-4">
    <header class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-semibold text-fg">Orchestrations</h1>
        <p class="text-sm text-fg-muted">Generic correlations, immutable execution epochs, adapters, and provider effects.</p>
      </div>
      <div class="flex gap-1 rounded border border-border bg-surface p-1">
        <button v-for="item in modes" :key="item" class="rounded px-3 py-1.5 text-sm" :class="mode === item ? 'bg-accent text-white' : 'text-fg-muted'" @click="switchMode(item)">{{ item }}</button>
      </div>
    </header>

    <p v-if="store.error" class="rounded border border-danger/40 bg-danger/10 p-3 text-sm text-danger">{{ store.error }}</p>

    <template v-if="mode === 'Instances'">
      <div class="flex flex-wrap gap-2">
        <select v-model="filters.status" class="input" @change="refreshInstances"><option value="">All statuses</option><option v-for="item in statuses" :key="item" :value="item">{{ item }}</option></select>
        <input v-model="filters.scope" class="input" placeholder="Scope" @keyup.enter="refreshInstances" />
        <input v-model="filters.correlation_key" class="input min-w-56" placeholder="Correlation key" @keyup.enter="refreshInstances" />
        <input v-model="filters.pipeline_id" class="input min-w-56" placeholder="Pipeline ID" @keyup.enter="refreshInstances" />
        <input v-model="filters.adapter_id" class="input min-w-56" placeholder="Adapter ID" @keyup.enter="refreshInstances" />
        <button class="button" :disabled="store.loading" @click="refreshInstances">Refresh</button>
      </div>

      <div class="grid min-h-0 flex-1 gap-3 lg:grid-cols-[22rem_minmax(0,1fr)]">
        <aside class="min-h-0 overflow-auto rounded border border-border bg-surface">
          <button v-for="binding in store.bindings" :key="binding.id" class="block w-full border-b border-border p-3 text-left hover:bg-surface-raised" :class="{ 'bg-surface-raised': binding.id === store.selectedId }" @click="store.select(binding.id)">
            <div class="flex items-center justify-between gap-2"><span class="truncate font-medium text-fg">{{ binding.correlation_key }}</span><span class="rounded bg-surface-raised px-2 py-0.5 text-xs text-fg-muted">{{ binding.status }}</span></div>
            <div class="mt-1 truncate text-xs text-fg-muted">{{ binding.scope }} · generation {{ binding.generation }} · epoch {{ binding.current_epoch }}</div>
          </button>
          <p v-if="!store.loading && store.bindings.length === 0" class="p-4 text-sm text-fg-muted">No orchestrations match these filters.</p>
        </aside>

        <main v-if="store.selected" class="min-h-0 overflow-auto rounded border border-border bg-surface p-4">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div><h2 class="text-lg font-semibold text-fg">{{ store.selected.correlation_key }}</h2><div class="mt-2 flex flex-wrap gap-2 text-xs text-fg-muted"><span v-for="chip in instanceChips" :key="chip" class="rounded bg-surface-raised px-2 py-1">{{ chip }}</span></div></div>
            <div class="flex flex-wrap gap-2"><button v-for="(_, name) in store.selected.policy.intents" :key="name" class="button" @click="openIntent(String(name))">{{ name }}</button><button v-if="isSelectedTerminal" class="button" @click="openRequeue">Requeue generation</button></div>
          </div>
          <nav class="mt-5 flex flex-wrap gap-1 border-b border-border"><button v-for="tab in instanceTabs" :key="tab" class="px-3 py-2 text-sm" :class="tab === activeInstanceTab ? 'border-b-2 border-accent text-fg' : 'text-fg-muted'" @click="activeInstanceTab = tab">{{ tab }}</button></nav>
          <div class="mt-4">
            <div v-if="activeInstanceTab === 'Timeline'" class="space-y-2">
              <article v-for="event in store.events" :key="event.id" class="rounded border border-border p-3 text-sm"><div class="flex justify-between gap-3"><strong>#{{ event.sequence }} {{ event.winner || "observed" }}</strong><span class="text-fg-muted">{{ event.disposition }}</span></div><p class="mt-1 text-xs text-fg-muted">matched: {{ event.matched_intents.join(", ") || "none" }} · suppressed: {{ event.suppressed_intents.join(", ") || "none" }}</p><pre class="mt-2 overflow-auto text-xs">{{ pretty(event.detail) }}</pre></article>
            </div>
            <div v-else-if="activeInstanceTab === 'Epochs'" class="space-y-2">
              <article v-for="epoch in store.epochs" :key="epoch.id" class="rounded border border-border p-3 text-sm">
                <div class="flex flex-wrap items-start justify-between gap-2">
                  <div><strong>Epoch {{ epoch.epoch }}</strong><p class="text-xs text-fg-muted">starts at {{ epoch.start_member || "pipeline entry" }} · {{ epoch.reason }}</p></div>
                  <span class="rounded bg-surface-raised px-2 py-1 text-xs">{{ epoch.status }}</span>
                </div>
                <button v-if="epoch.pipeline_run_id" class="button mt-3" @click="openPipelineRun(epoch.pipeline_run_id)">Open pipeline run</button>
                <details class="mt-2"><summary class="cursor-pointer text-xs text-fg-muted">Epoch parameters</summary><pre class="mt-2 overflow-auto text-xs">{{ pretty(epoch.parameters) }}</pre></details>
              </article>
              <p v-if="store.epochs.length === 0" class="text-sm text-fg-muted">No execution epoch has been created.</p>
            </div>
            <pre v-else-if="activeInstanceTab === 'Evidence'" class="overflow-auto text-xs">{{ pretty(store.evidence) }}</pre>
            <pre v-else-if="activeInstanceTab === 'Resources'" class="overflow-auto text-xs">{{ pretty(store.selected.resources) }}</pre>
            <pre v-else-if="activeInstanceTab === 'Budgets'" class="overflow-auto text-xs">{{ pretty({ consumed: store.selected.budgets, policy: store.selected.policy.budgets }) }}</pre>
            <div v-else-if="activeInstanceTab === 'Operations'" class="space-y-2">
              <article v-for="operation in store.operations" :key="operation.id" class="rounded border border-border p-3 text-sm"><div class="flex flex-wrap items-start justify-between gap-2"><div><strong>{{ operation.provider }}.{{ operation.action }}</strong><p class="text-xs text-fg-muted">{{ operation.semantics }} · attempt {{ operation.attempt }} · epoch {{ operation.epoch }}</p></div><span class="rounded bg-surface-raised px-2 py-1 text-xs">{{ operation.status }}<template v-if="operation.ambiguous"> · ambiguous</template></span></div><code class="mt-2 block break-all text-xs text-fg-muted">{{ operation.operation_key }}</code><div v-if="operation.status === 'waiting' || operation.ambiguous" class="mt-3 flex gap-2"><button class="button" @click="openResolution(operation, 'succeeded')">Mark succeeded</button><button class="button" @click="openResolution(operation, 'failed')">Mark failed</button><button v-if="operation.semantics !== 'at_least_once'" class="button" @click="openResolution(operation, 'retry')">Retry safely</button></div></article>
              <p v-if="store.operations.length === 0" class="text-sm text-fg-muted">No binding-scoped provider operations.</p>
            </div>
            <div v-else-if="activeInstanceTab === 'Workspaces'" class="space-y-2">
              <article v-for="workspace in store.workspaces" :key="workspace.id" class="rounded border border-border p-3 text-sm">
                <div class="flex flex-wrap items-start justify-between gap-2">
                  <div><strong>{{ workspace.scope }}</strong><p class="text-xs text-fg-muted">attempt {{ workspace.attempt }} · {{ workspace.worker_instance_id }}</p></div>
                  <span class="rounded bg-surface-raised px-2 py-1 text-xs">{{ workspace.status }} · CAS {{ workspace.version }}</span>
                </div>
                <code class="mt-2 block break-all text-xs text-fg-muted">{{ workspace.local_key }}</code>
                <p class="mt-2 text-xs text-fg-muted">lease until {{ workspace.leased_until }}</p>
                <details v-if="workspace.evidence !== null" class="mt-2"><summary class="cursor-pointer text-xs">Evidence</summary><pre class="mt-2 overflow-auto text-xs">{{ pretty(workspace.evidence) }}</pre></details>
              </article>
              <p v-if="store.workspaces.length === 0" class="text-sm text-fg-muted">No workspaces allocated for this generation.</p>
            </div>
            <pre v-else-if="activeInstanceTab === 'Commands'" class="overflow-auto text-xs">{{ pretty(store.commands) }}</pre>
            <pre v-else class="overflow-auto text-xs">{{ pretty(store.selected) }}</pre>
          </div>
        </main>
        <main v-else class="rounded border border-border bg-surface p-6 text-sm text-fg-muted">Select an orchestration.</main>
      </div>
    </template>

    <template v-else>
      <div class="flex flex-wrap items-center gap-2"><button class="button" @click="openAdapterForm()">New adapter</button><button class="button" :disabled="store.loading" @click="refreshAdapters">Refresh</button><button class="button" @click="checkHost">Host health</button><button class="button" @click="reloadHost">Reload plugins</button><pre v-if="hostResult" class="max-w-xl overflow-auto text-xs">{{ pretty(hostResult) }}</pre></div>
      <div class="grid min-h-0 flex-1 gap-3 lg:grid-cols-[22rem_minmax(0,1fr)]">
        <aside class="min-h-0 overflow-auto rounded border border-border bg-surface">
          <div class="border-b border-border p-3"><h3 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">Adapter kinds</h3><div class="mt-2 grid gap-1"><div v-for="entry in adapterCatalog" :key="`${entry.metadata.kind}:${entry.origin}`" class="rounded bg-surface-raised px-2 py-1 text-xs" :title="entry.error || entry.metadata.description || ''"><div class="flex items-center justify-between gap-2"><span>{{ entry.metadata.display_name }} v{{ entry.metadata.version }}</span><span :class="entry.healthy ? 'text-success' : 'text-danger'">{{ entry.healthy ? 'healthy' : 'error' }}</span></div><p class="truncate text-fg-muted">{{ entry.origin }}</p><p v-if="entry.error" class="mt-1 text-danger">{{ entry.error }}</p></div></div></div>
          <button v-for="adapter in store.adapters" :key="adapter.id" class="block w-full border-b border-border p-3 text-left hover:bg-surface-raised" :class="{ 'bg-surface-raised': adapter.id === store.selectedAdapterId }" @click="store.selectAdapter(adapter.id)"><div class="flex justify-between gap-2"><span class="truncate font-medium">{{ adapter.name }}</span><span class="text-xs" :class="adapter.enabled ? 'text-success' : 'text-fg-muted'">{{ adapter.enabled ? 'enabled' : 'disabled' }}</span></div><p class="mt-1 text-xs text-fg-muted">{{ adapter.kind }} · revision {{ adapter.current_revision }}</p></button>
          <p v-if="store.adapters.length === 0" class="p-4 text-sm text-fg-muted">No adapters configured for this organization.</p>
        </aside>

        <main v-if="store.selectedAdapter" class="min-h-0 overflow-auto rounded border border-border bg-surface p-4">
          <div class="flex flex-wrap justify-between gap-3"><div><h2 class="text-lg font-semibold">{{ store.selectedAdapter.name }}</h2><p class="text-sm text-fg-muted">{{ selectedKind?.display_name || store.selectedAdapter.kind }} · immutable revision {{ store.selectedAdapter.current_revision }}</p></div><div class="flex flex-wrap gap-2"><button class="button" @click="copyWebhook">Copy webhook URL</button><button class="button" @click="openAdapterForm(store.selectedAdapter)">Edit</button><button class="button" @click="openAdapterForm(store.selectedAdapter, true)">Clone</button><button class="button" @click="toggleSelectedAdapter">{{ store.selectedAdapter.enabled ? 'Disable' : 'Enable' }}</button><button class="button" :disabled="store.selectedAdapter.has_admitted_binding" @click="removeSelectedAdapter">Delete</button></div></div>
          <code class="mt-3 block break-all rounded bg-surface-raised p-2 text-xs">{{ webhookPath }}</code><p v-if="store.selectedAdapter.has_admitted_binding" class="mt-2 text-xs text-fg-muted">Identity extraction is locked because this adapter has admitted a correlation.</p>
          <div v-if="selectedKind" class="mt-3 grid gap-2 text-xs md:grid-cols-3"><div><strong>Capabilities</strong><p class="text-fg-muted">{{ selectedKind.capabilities.join(', ') || 'normalize' }}</p></div><div><strong>Canonical events</strong><p class="text-fg-muted">{{ selectedKind.event_names.join(', ') || 'provider-defined' }}</p></div><div><strong>Canonical pointers</strong><p class="break-all text-fg-muted">{{ selectedKind.canonical_pointers.join(', ') || 'provider-defined' }}</p></div></div>
          <nav class="mt-5 flex gap-1 border-b border-border"><button v-for="tab in adapterTabs" :key="tab" class="px-3 py-2 text-sm" :class="tab === activeAdapterTab ? 'border-b-2 border-accent' : 'text-fg-muted'" @click="activeAdapterTab = tab">{{ tab }}</button></nav>
          <pre v-if="activeAdapterTab === 'Configuration'" class="mt-4 overflow-auto text-xs">{{ pretty(currentAdapterRevision) }}</pre><pre v-else-if="activeAdapterTab === 'Revisions'" class="mt-4 overflow-auto text-xs">{{ pretty(store.adapterRevisions) }}</pre>
          <div v-else class="mt-4 grid gap-3">
            <label class="text-sm">Headers JSON<textarea v-model="testHeaders" class="input mt-1 min-h-24 w-full font-mono text-xs" /></label>
            <label class="text-sm">Sample request body<textarea v-model="testBody" class="input mt-1 min-h-40 w-full font-mono text-xs" /></label>
            <button class="button w-fit" @click="runTest">Verify, normalize, and preview routes</button>
            <section v-if="testResult" class="grid gap-3">
              <div class="flex items-center gap-2 rounded border border-border p-3 text-sm">
                <span class="rounded px-2 py-1 text-xs" :class="testResult.verified ? 'bg-success/15 text-success' : 'bg-danger/15 text-danger'">{{ testResult.verified ? "Verified" : "Rejected" }}</span>
                <span class="text-fg-muted">{{ testResult.events.length }} normalized event(s)</span>
              </div>
              <ul v-if="testResult.errors.length" class="rounded border border-danger/40 bg-danger/10 p-3 text-sm text-danger"><li v-for="error in testResult.errors" :key="error">{{ error }}</li></ul>
              <article v-for="preview in testResult.previews" :key="preview.delivery_id" class="rounded border border-border p-3">
                <div class="flex flex-wrap items-start justify-between gap-2"><div><strong>{{ preview.event_type }}</strong><p class="text-xs text-fg-muted">{{ preview.scope }}/{{ preview.correlation_key }}</p></div><span class="rounded bg-surface-raised px-2 py-1 text-xs">{{ preview.lifecycle }}</span></div>
                <ul v-if="preview.validation_errors.length" class="mt-3 rounded bg-warning/10 p-2 text-xs text-warning"><li v-for="error in preview.validation_errors" :key="error">{{ error }}</li></ul>
                <div v-for="match in preview.pipeline_matches" :key="match.pipeline_id" class="mt-3 rounded bg-surface-raised p-3 text-sm">
                  <div class="flex flex-wrap justify-between gap-2"><strong>{{ match.pipeline_name }}</strong><span class="text-xs text-fg-muted">{{ match.managed ? "managed" : "unmanaged" }}</span></div>
                  <p class="mt-2 text-xs text-fg-muted">Matched actions: {{ match.routes.map((route) => route.action).join(", ") }}</p>
                  <p class="mt-1 text-xs">Candidate intents: {{ match.candidate_intents.join(", ") || "none" }}</p>
                  <p v-if="match.winner" class="mt-1 text-xs"><strong>Winner:</strong> {{ match.winner }}<template v-if="match.suppressed_intents.length"> · suppressed {{ match.suppressed_intents.join(", ") }}</template></p>
                  <details class="mt-2"><summary class="cursor-pointer text-xs text-fg-muted">Matched route details</summary><pre class="mt-2 overflow-auto text-xs">{{ pretty(match.routes) }}</pre></details>
                </div>
              </article>
              <details><summary class="cursor-pointer text-xs text-fg-muted">Raw normalized response</summary><pre class="mt-2 overflow-auto rounded bg-surface-raised p-3 text-xs">{{ pretty(testResult) }}</pre></details>
            </section>
          </div>
        </main>
        <main v-else class="rounded border border-border bg-surface p-6 text-sm text-fg-muted">Select an adapter or create one.</main>
      </div>
    </template>

    <div v-if="intentName" class="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" @click.self="intentName = null"><form class="w-full max-w-md rounded border border-border bg-surface p-4 shadow-xl" @submit.prevent="submitIntent"><h2 class="font-semibold">Dispatch {{ intentName }}</h2><label class="mt-3 block text-sm text-fg-muted">Reason</label><textarea v-model="reason" required class="input mt-1 min-h-24 w-full" /><label class="mt-3 block text-sm text-fg-muted">Payload JSON</label><textarea v-model="intentPayload" class="input mt-1 min-h-28 w-full font-mono text-xs" /><p v-if="intentPayloadError" class="mt-2 text-sm text-danger">{{ intentPayloadError }}</p><div class="mt-4 flex justify-end gap-2"><button type="button" class="button" @click="intentName = null">Cancel</button><button class="button" type="submit">Dispatch</button></div></form></div>
    <div v-if="requeueOpen" class="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" @click.self="requeueOpen = false"><form class="w-full max-w-md rounded border border-border bg-surface p-4 shadow-xl" @submit.prevent="submitRequeue"><h2 class="font-semibold">Requeue next generation</h2><p class="mt-1 text-xs text-fg-muted">The next generation snapshots the current immutable pipeline and adapter revisions.</p><label class="mt-3 block text-sm text-fg-muted">Reason</label><textarea v-model="reason" required class="input mt-1 min-h-24 w-full" /><div class="mt-4 flex justify-end gap-2"><button type="button" class="button" @click="requeueOpen = false">Cancel</button><button class="button" type="submit">Requeue</button></div></form></div>
    <div v-if="resolvingOperation" class="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" @click.self="resolvingOperation = null"><form class="w-full max-w-lg rounded border border-border bg-surface p-4" @submit.prevent="submitResolution"><h2 class="font-semibold">Resolve {{ resolvingOperation.provider }}.{{ resolvingOperation.action }}</h2><p class="mt-1 text-xs text-fg-muted">{{ resolution }} · {{ resolvingOperation.semantics }}</p><label class="mt-3 block text-sm">Reason<textarea v-model="resolutionReason" required class="input mt-1 min-h-20 w-full" /></label><label class="mt-3 block text-sm">Receipt JSON<textarea v-model="resolutionReceipt" class="input mt-1 min-h-28 w-full font-mono text-xs" /></label><div class="mt-4 flex justify-end gap-2"><button type="button" class="button" @click="resolvingOperation = null">Cancel</button><button class="button" type="submit">Apply resolution</button></div></form></div>
    <div v-if="adapterFormOpen" class="fixed inset-0 z-50 grid place-items-center overflow-auto bg-black/50 p-4" @click.self="adapterFormOpen = false"><form class="my-8 w-full max-w-2xl rounded border border-border bg-surface p-4" @submit.prevent="saveAdapter"><h2 class="font-semibold">{{ editingAdapterId ? 'Edit adapter' : 'New adapter' }}</h2><div class="mt-3 grid gap-3 md:grid-cols-2"><label class="text-sm">Name<input v-model="adapterForm.name" required class="input mt-1 w-full" /></label><label class="text-sm">Kind<select v-model="adapterForm.kind" required class="input mt-1 w-full" :disabled="!!editingAdapterId" @change="initializeKind"><option value="" disabled>Select a loaded kind</option><option v-for="kind in store.adapterKinds" :key="kind.kind" :value="kind.kind">{{ kind.display_name }} v{{ kind.version }}</option></select></label></div>
      <div v-if="formKind" class="mt-4 grid gap-3"><p class="text-sm text-fg-muted">{{ formKind.description }}</p><label v-for="field in configurationFields" :key="field.name" class="text-sm"><span>{{ field.name }}<template v-if="field.required"> *</template></span><input v-if="fieldInputType(field) !== 'checkbox'" :type="fieldInputType(field)" class="input mt-1 w-full" :value="adapterForm.configuration[field.name] ?? ''" @input="updateConfigField(field, $event)" /><input v-else type="checkbox" class="ml-2" :checked="Boolean(adapterForm.configuration[field.name])" @change="updateConfigField(field, $event)" /><small v-if="field.description" class="mt-1 block text-fg-muted">{{ field.description }}</small></label><label v-for="field in secretFields" :key="field.name" class="text-sm">{{ field.name }} Secret<template v-if="field.required"> *</template><select v-model="adapterForm.secret_bindings[field.name]" class="input mt-1 w-full" :required="field.required"><option value="">Select stored Secret</option><option v-for="secret in selectableSecrets" :key="secret.id" :value="secret.id">{{ secret.scope }}/{{ secret.name }}</option></select><small v-if="field.description" class="mt-1 block text-fg-muted">{{ field.description }}</small></label><label class="text-sm">Identity extraction JSON<textarea v-model="identityText" class="input mt-1 min-h-28 w-full font-mono text-xs" :disabled="identityLocked" /></label></div><div class="mt-4 flex justify-end gap-2"><button type="button" class="button" @click="adapterFormOpen = false">Cancel</button><button class="button" type="submit">Save immutable revision</button></div></form></div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, shallowRef } from "vue";
import type {
  AdapterConfigurationField,
  AdapterDefinition,
  AdapterKindCatalogEntry,
  AdapterKindMetadata,
  AdapterRevision,
  ExternalOperation,
  JsonValue,
} from "../../core/domain/models";
import { fetchAdapterHealth, fetchAdapterKinds, reloadAdapterHost } from "../../core/services/orchestrations";
import { useAppStore } from "../adapters/pinia/app";
import { useOrchestrationsStore } from "../adapters/pinia/orchestrations";
import { usePipelineRunsStore } from "../adapters/pinia/pipeline-runs";
import { useSecretsStore } from "../adapters/pinia/secrets";

const store = useOrchestrationsStore();
const app = useAppStore();
const pipelineRuns = usePipelineRunsStore();
const secrets = useSecretsStore();
const modes = ["Instances", "Adapters"] as const;
type Mode = (typeof modes)[number];
const mode = ref<Mode>("Instances");
const statuses = ["pending", "running", "waiting", "suspended", "completed", "failed", "terminated"];
const instanceTabs = ["Timeline", "Epochs", "Evidence", "Resources", "Budgets", "Operations", "Workspaces", "Commands", "Raw"];
const adapterTabs = ["Configuration", "Revisions", "Test"];
const activeInstanceTab = ref("Timeline");
const activeAdapterTab = ref("Configuration");
const filters = reactive({ status: "", scope: "", correlation_key: "", pipeline_id: "", adapter_id: "" });
const intentName = ref<string | null>(null);
const intentPayload = ref("{}");
const intentPayloadError = ref<string | null>(null);
const requeueOpen = ref(false);
const reason = ref("");
const resolvingOperation = ref<ExternalOperation | null>(null);
const resolution = ref<"succeeded" | "failed" | "retry">("succeeded");
const resolutionReason = ref("");
const resolutionReceipt = ref("null");
const hostResult = ref<unknown>(null);
const adapterCatalog = shallowRef<AdapterKindCatalogEntry[]>([]);
const testHeaders = ref("{}");
const testBody = ref("{}");
interface AdapterTestRoute {
  action: string;
  intent?: string | null;
  predicates: unknown[];
}
interface AdapterTestPipelineMatch {
  pipeline_id: string;
  pipeline_name: string;
  managed: boolean;
  routes: AdapterTestRoute[];
  candidate_intents: string[];
  winner?: string | null;
  suppressed_intents: string[];
}
interface AdapterEventPreview {
  delivery_id: string;
  scope: string;
  correlation_key: string;
  event_type: string;
  lifecycle: string;
  pipeline_matches: AdapterTestPipelineMatch[];
  validation_errors: string[];
}
interface AdapterTestResult {
  verified: boolean;
  events: unknown[];
  errors: string[];
  previews: AdapterEventPreview[];
}
const testResult = ref<AdapterTestResult | null>(null);
const adapterFormOpen = ref(false);
const editingAdapterId = ref<string | null>(null);
const identityText = ref("{}");
interface AdapterFormState {
  name: string;
  kind: string;
  configuration: Record<string, JsonValue>;
  secret_bindings: Record<string, string>;
}
const adapterForm = reactive<AdapterFormState>({
  name: "",
  kind: "",
  configuration: {},
  secret_bindings: {},
});

const selectedKind = computed<AdapterKindMetadata | undefined>(
  () => store.adapterKinds.find((kind) => kind.kind === store.selectedAdapter?.kind),
);
const formKind = computed<AdapterKindMetadata | undefined>(
  () => store.adapterKinds.find((kind) => kind.kind === adapterForm.kind),
);
const configurationFields = computed(() => formKind.value?.fields.filter((field) => !field.secret) ?? []);
const secretFields = computed(() => formKind.value?.fields.filter((field) => field.secret) ?? []);
const selectableSecrets = computed(() => secrets.secretEntries.filter((secret) => Boolean(secret.id)));
const currentAdapterRevision = computed<AdapterRevision | undefined>(
  () => store.adapterRevisions.find(
    (revision) => revision.revision === store.selectedAdapter?.current_revision,
  ) ?? store.adapterRevisions[0],
);
const webhookPath = computed(() => store.selectedAdapter ? `/webhooks/orchestration/${store.selectedAdapter.endpoint_identity}` : "");
const identityLocked = computed(() => Boolean(editingAdapterId.value && store.selectedAdapter?.has_admitted_binding));
const isSelectedTerminal = computed(() => Boolean(
  store.selected && ["completed", "failed", "terminated"].includes(store.selected.status),
));
const instanceChips = computed(() => store.selected ? [
  `generation ${String(store.selected.generation)}`,
  `epoch ${String(store.selected.current_epoch)}`,
  `phase ${store.selected.current_phase ?? "—"}`,
  `attempt ${String(store.selected.current_attempt)}`,
  `CAS ${String(store.selected.version)}`,
  `pipeline revision ${String(store.selected.pipeline_revision)}`,
  ...(store.selected.adapter_id
    ? [`adapter revision ${String(store.selected.adapter_revision ?? "—")}`]
    : []),
] : []);

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function jsonObject(value: JsonValue | undefined): Record<string, JsonValue> {
  return value && typeof value === "object" && !Array.isArray(value) ? { ...value } : {};
}

function parseJson(value: string): unknown {
  const parsed: unknown = JSON.parse(value);

  return parsed;
}

function refreshInstances(): void {
  void store.refresh(Object.fromEntries(Object.entries(filters).filter(([, value]) => value)));
}

async function refreshAdapters(): Promise<void> {
  const [, , catalog] = await Promise.all([
    store.refreshAdapters(),
    secrets.refreshSecrets(),
    fetchAdapterKinds(),
  ]);
  adapterCatalog.value = catalog;
}

function switchMode(next: Mode): void {
  mode.value = next;

  if (next === "Instances") {
    refreshInstances();
  } else {
    void refreshAdapters();
  }
}

function openIntent(name: string): void {
  intentName.value = name;
  reason.value = "";
  intentPayload.value = "{}";
  intentPayloadError.value = null;
}

function openRequeue(): void {
  reason.value = "";
  requeueOpen.value = true;
}

async function submitIntent(): Promise<void> {
  if (!intentName.value || !reason.value.trim()) {
    return;
  }

  let payload: unknown;

  try {
    payload = parseJson(intentPayload.value || "{}");
  } catch (cause) {
    intentPayloadError.value = cause instanceof Error ? cause.message : "Payload must be valid JSON.";
    return;
  }

  await store.dispatch(intentName.value, reason.value.trim(), payload);
  intentName.value = null;
}

async function openPipelineRun(id: string): Promise<void> {
  await pipelineRuns.selectRun(id);
  app.activeTab = "PipelineRuns";
}

async function submitRequeue(): Promise<void> {
  if (!reason.value.trim()) {
    return;
  }

  await store.requeue(reason.value.trim());
  requeueOpen.value = false;
}

function openResolution(operation: ExternalOperation, next: typeof resolution.value): void {
  resolvingOperation.value = operation;
  resolution.value = next;
  resolutionReason.value = "";
  resolutionReceipt.value = "null";
}

async function submitResolution(): Promise<void> {
  if (!resolvingOperation.value || !resolutionReason.value.trim()) {
    return;
  }

  const receipt = parseJson(resolutionReceipt.value || "null");

  await store.resolveOperation(
    resolvingOperation.value,
    resolution.value,
    resolutionReason.value.trim(),
    receipt,
  );
  resolvingOperation.value = null;
}

async function checkHost(): Promise<void> {
  hostResult.value = await fetchAdapterHealth();
}

async function reloadHost(): Promise<void> {
  hostResult.value = await reloadAdapterHost();
  await refreshAdapters();
}

async function copyWebhook(): Promise<void> {
  await navigator.clipboard.writeText(webhookPath.value);
}

async function toggleSelectedAdapter(): Promise<void> {
  if (store.selectedAdapter) {
    await store.toggleAdapter(store.selectedAdapter);
  }
}

async function removeSelectedAdapter(): Promise<void> {
  if (store.selectedAdapter) {
    await store.removeAdapter(store.selectedAdapter);
  }
}

function toBase64(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";

  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  return btoa(binary);
}

async function runTest(): Promise<void> {
  if (!store.selectedAdapter) {
    return;
  }

  const parsed = parseJson(testHeaders.value || "{}");

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("Headers must be a JSON object");
  }

  const headers: Record<string, string> = {};

  for (const [name, value] of Object.entries(parsed)) {
    if (typeof value !== "string") {
      throw new Error(`Header ${name} must be a string`);
    }

    headers[name] = value;
  }

  testResult.value = await store.runAdapterTest(
    store.selectedAdapter.id,
    headers,
    toBase64(testBody.value),
  ) as AdapterTestResult;
}

function initializeKind(): void {
  adapterForm.configuration = {};
  adapterForm.secret_bindings = {};

  for (const field of formKind.value?.fields ?? []) {
    if (!field.secret) {
      adapterForm.configuration[field.name] = field.default as JsonValue;
    }
  }
}

function openAdapterForm(adapter?: AdapterDefinition, clone = false): void {
  const revision: AdapterRevision | undefined = adapter
    ? currentAdapterRevision.value
    : undefined;
  const firstKind = store.adapterKinds.at(0);

  editingAdapterId.value = adapter && !clone ? adapter.id : null;
  adapterForm.name = adapter ? `${adapter.name}${clone ? " copy" : ""}` : "";
  adapterForm.kind = adapter ? adapter.kind : (firstKind ? firstKind.kind : "");
  adapterForm.configuration = revision ? jsonObject(revision.configuration) : {};
  adapterForm.secret_bindings = revision ? { ...revision.secret_bindings } : {};
  identityText.value = pretty(revision?.identity_configuration ?? {});

  if (!revision) {
    initializeKind();
  }

  adapterFormOpen.value = true;
}

function fieldInputType(field: AdapterConfigurationField): string {
  const rendered = JSON.stringify(field.value_type).toLowerCase();

  if (rendered.includes("bool")) {
    return "checkbox";
  }

  return rendered.includes("int") || rendered.includes("number") || rendered.includes("float")
    ? "number"
    : "text";
}

function updateConfigField(field: AdapterConfigurationField, event: Event): void {
  const target = event.target as HTMLInputElement;
  const kind = fieldInputType(field);

  adapterForm.configuration[field.name] = kind === "checkbox"
    ? target.checked
    : kind === "number" && target.value !== ""
      ? Number(target.value)
      : target.value;
}

async function saveAdapter(): Promise<void> {
  const kind = formKind.value;

  if (!kind) {
    return;
  }

  const identity = parseJson(identityText.value || "{}");
  const bindings = Object.fromEntries(
    Object.entries(adapterForm.secret_bindings).filter(([, value]) => value),
  );

  await store.saveAdapter({
    name: adapterForm.name.trim(),
    kind: kind.kind,
    kind_version: kind.version,
    configuration: adapterForm.configuration,
    secret_bindings: bindings,
    identity_configuration: identity,
    ...(editingAdapterId.value && store.selectedAdapter
      ? { expected_revision: store.selectedAdapter.current_revision }
      : {}),
  }, editingAdapterId.value ?? undefined);
  adapterFormOpen.value = false;
}

onMounted(refreshInstances);
</script>
