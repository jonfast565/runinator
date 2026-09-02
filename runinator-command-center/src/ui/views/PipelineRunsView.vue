<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.pipeline-runs.split"
      :initial-first-pct="28"
      :min-first="340"
      :min-second="720"
      collapsible-first
      first-label="Pipeline Runs"
      first-icon="runs"
      mobile-mode="toggle"
      :mobile-detail-active="!!store.selectedRunId"
    >
      <template #first>
        <div class="panel min-h-0">
          <PanelHeader
            title="Pipeline Runs"
            icon="runs"
            eyebrow="Pipeline execution"
            description="First-class pipeline executions and the member workflow runs they orchestrate."
          >
            <button class="btn" :disabled="store.loading" @click="store.refresh">
              <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing pipeline runs" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </PanelHeader>
          <form class="flex gap-2" @submit.prevent="startRun">
            <select v-model="selectedPipelineId" class="input flex-1" required>
              <option value="" disabled>Choose a pipeline…</option>
              <option
                v-for="pipeline in store.pipelines"
                :key="pipeline.id ?? ''"
                :value="pipeline.id ?? ''"
              >
                {{ pipeline.name }}
              </option>
            </select>
            <button class="btn btn-primary" type="submit" :disabled="starting">
              <Icon name="runs" />
              <span>Start run</span>
            </button>
          </form>
          <div class="mb-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard label="Visible" :value="store.runs.length" />
            <MetricCard label="Active" :value="activeRunCount" />
            <MetricCard label="Selected" :value="selectedRunLabel" />
          </div>
          <p v-if="store.error" class="error m-0 text-xs">{{ store.error }}</p>
          <EmptyState
            v-if="store.loading && !store.runs.length"
            compact
            loading
            title="Loading pipeline runs"
          />
          <EmptyState
            v-else-if="!store.runs.length"
            compact
            icon="runs"
            title="No pipeline runs yet"
            description="Start a pipeline above, or trigger one via a cron/chained pipeline trigger."
          />
          <template v-else>
            <BulkActionBar
              class="mb-2"
              noun="pipeline run"
              :count="selection.count.value"
              :actions="bulkActions"
              :busy="bulkBusy"
              @run="runBulkAction"
              @clear="selection.clear"
            />
            <div
              class="table-scroll min-h-0 flex-1"
              :class="{ 'opacity-60 transition-opacity duration-100': store.loading }"
            >
              <RunTable
                :runs="runRows"
                :selected-run-id="store.selectedRunId"
                :workflow-names="pipelineNames"
                show-workflow
                entity-label="Pipeline"
                list-mode
                selectable
                :selected-run-ids="selection.selectedKeys.value as string[]"
                :all-selected="selection.allSelected.value"
                :some-selected="selection.someSelected.value"
                deletable
                @select="onSelectRun"
                @toggle-row="selection.toggle"
                @toggle-all="selection.toggleAll"
                @delete="deletePipelineRunFromList"
              />
            </div>
          </template>
        </div>
      </template>

      <template #second>
        <div class="flex min-h-0 flex-1 flex-col">
          <MobileBackBar label="Back to pipeline runs" @back="store.selectedRunId = null" />
          <div v-if="!store.detail" class="panel min-h-0">
            <EmptyState
              icon="branch"
              title="Select a pipeline run"
              description="Pick a run on the left to see its member workflow runs and their status."
            />
          </div>
          <div v-else class="panel details flex min-h-0 flex-col gap-3 overflow-auto">
            <div class="flex items-baseline justify-between gap-2">
              <div class="flex items-center gap-2.5">
                <h2 class="m-0 text-base font-semibold text-fg">
                  {{ pipelineName(store.detail.run.pipeline_id) }}
                </h2>
                <StatusBadge :status="store.detail.run.status" />
              </div>
              <button
                v-if="managedBindingId"
                class="btn btn-primary btn-sm"
                @click="openOrchestration"
              >
                <Icon name="branch" />
                <span>Open orchestration</span>
              </button>
              <button
                v-if="!managedBindingId"
                class="btn btn-sm"
                :disabled="
                  store.detail.run.status === 'paused' ||
                  !isActiveRunStatus(store.detail.run.status) ||
                  runControlBusy
                "
                title="Pause after the current member workflow run finishes"
                @click="pauseRun(store.detail.run.id)"
              >
                <Icon name="pause" />
                <span>Pause</span>
              </button>
              <button
                v-if="!managedBindingId"
                class="btn btn-sm"
                :disabled="store.detail.run.status !== 'paused' || runControlBusy"
                title="Resume a paused pipeline run"
                @click="resumeRun(store.detail.run.id)"
              >
                <Icon name="play" />
                <span>Resume</span>
              </button>
              <button
                v-if="!managedBindingId"
                class="btn btn-danger btn-sm"
                :disabled="!isActiveRunStatus(store.detail.run.status) || runControlBusy"
                title="Cancel pipeline run immediately"
                @click="cancelRun(store.detail.run.id)"
              >
                <Icon name="reject" />
                <span>Cancel</span>
              </button>
              <button
                v-if="!managedBindingId"
                class="btn btn-danger btn-sm"
                :disabled="runControlBusy"
                title="Permanently delete run"
                @click="deleteRun(store.detail.run.id)"
              >
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
            <dl class="m-0 grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-2">
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Run</dt>
                <dd class="m-0 text-[13px] text-fg">#{{ store.detail.run.id }}</dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Source</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ store.detail.run.trigger_source_kind ?? "-" }}
                </dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Started</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ formatDate(store.detail.run.started_at) }}
                </dd>
              </div>
              <div
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Finished</dt>
                <dd class="m-0 text-[13px] text-fg">
                  {{ formatDate(store.detail.run.finished_at) }}
                </dd>
              </div>
              <div
                v-if="store.detail.run.start_member"
                class="grid gap-0.5 rounded-md border border-border-subtle bg-surface-subtle px-2.5 py-2"
              >
                <dt class="text-xs text-fg-muted">Starting member</dt>
                <dd class="m-0 text-[13px] text-fg">{{ store.detail.run.start_member }}</dd>
              </div>
            </dl>
            <p v-if="store.detail.run.message" class="m-0 text-[13px] text-fg-muted">
              {{ store.detail.run.message }}
            </p>

            <section
              v-if="managedBindingId"
              class="grid gap-3 rounded-md border border-accent/30 bg-accent/5 px-3 py-3"
            >
              <div class="flex flex-wrap items-center gap-2">
                <strong class="text-sm text-fg">Managed execution</strong>
                <span class="badge">Generation {{ managedBinding?.generation ?? "-" }}</span>
                <span class="badge">Epoch {{ store.detail.run.execution_epoch ?? "-" }}</span>
                <span class="badge">Phase {{ managedBinding?.current_phase ?? "-" }}</span>
                <span class="badge">Attempt {{ managedBinding?.current_attempt ?? "-" }}</span>
              </div>
              <p class="m-0 text-xs text-fg-muted">
                This immutable pipeline run is controlled by its correlated orchestration. Direct
                pause, cancel, retry, and deletion are disabled so the binding remains
                authoritative.
              </p>
              <div class="flex flex-wrap items-end gap-2">
                <label class="grid min-w-[260px] flex-1 gap-1 text-xs text-fg-muted">
                  <span>Reason for intent or emergency override</span>
                  <input
                    v-model="managedReason"
                    class="input"
                    required
                    minlength="3"
                    maxlength="500"
                    placeholder="Required operator reason"
                  />
                </label>
                <button
                  v-for="control in managedControls"
                  :key="control.name"
                  class="btn btn-sm"
                  :class="{ 'btn-danger': control.effect === 'terminate' }"
                  :disabled="!managedReason.trim() || managedIntentBusy"
                  @click="dispatchManagedIntent(control.name)"
                >
                  <span>{{ control.name }}</span>
                </button>
                <button class="btn btn-sm" @click="openOrchestration">Full timeline</button>
              </div>
              <div
                v-if="isPlatformAdmin"
                class="grid gap-2 rounded border border-warning-fg/30 bg-warning-bg px-3 py-2"
              >
                <p class="m-0 text-xs text-warning-fg">
                  Emergency controls bypass orchestration ownership. Each use requires the reason
                  above and is recorded as an out-of-band reducer event.
                </p>
                <div class="flex flex-wrap gap-2">
                  <button
                    class="btn btn-sm"
                    :disabled="!managedReason.trim() || runControlBusy"
                    @click="forceManagedControl('pause')"
                  >
                    Force pause
                  </button>
                  <button
                    class="btn btn-sm"
                    :disabled="!managedReason.trim() || runControlBusy"
                    @click="forceManagedControl('resume')"
                  >
                    Force resume
                  </button>
                  <button
                    class="btn btn-danger btn-sm"
                    :disabled="!managedReason.trim() || runControlBusy"
                    @click="forceManagedControl('cancel')"
                  >
                    Force cancel
                  </button>
                </div>
              </div>
            </section>

            <section class="grid gap-2 border-t border-border-subtle pt-3">
              <div class="flex items-baseline justify-between gap-2">
                <h2 class="m-0 text-base font-semibold text-fg">Execution Graph</h2>
              </div>
              <div
                class="h-[360px] min-h-[260px] overflow-hidden rounded-md border border-border-subtle bg-surface-subtle"
              >
                <PipelineCanvas :detail="store.detail" readonly @open-run="openMemberRunById" />
              </div>
            </section>

            <BrokerMessageLog
              :pipeline-run-id="store.detail.run.id"
              title="Broker messages for this pipeline run"
            />

            <section
              v-if="pendingInquiry"
              class="grid gap-2 rounded-md border border-warning-fg/30 bg-warning-bg px-3 py-2.5"
            >
              <h3 class="m-0 text-sm font-semibold text-warning-fg">
                Awaiting a decision (Inquire)
              </h3>
              <p class="m-0 text-[13px] text-warning-fg">
                <strong>{{ memberName(pendingInquiry.workflow_id) }}</strong> failed. Continue to
                fire its onward chain links and resume the pipeline, or abort to fail the pipeline
                run now.
              </p>
              <div class="flex gap-2">
                <button
                  v-if="managedBindingId"
                  class="btn btn-primary btn-sm"
                  @click="openOrchestration"
                >
                  <span>Resolve through orchestration</span>
                </button>
                <button
                  v-else
                  class="btn btn-primary btn-sm"
                  :disabled="resolving"
                  @click="resolveInquiry('continue')"
                >
                  <Icon name="runs" />
                  <span>Continue</span>
                </button>
                <button
                  v-if="!managedBindingId"
                  class="btn btn-danger btn-sm"
                  :disabled="resolving"
                  @click="resolveInquiry('abort')"
                >
                  <Icon name="reject" />
                  <span>Abort</span>
                </button>
              </div>
            </section>

            <section class="grid gap-2 border-t border-border-subtle pt-3">
              <div class="flex items-baseline justify-between gap-2">
                <h2 class="m-0 text-base font-semibold text-fg">Member Runs</h2>
                <span class="text-xs text-fg-muted">{{
                  store.detail.members.length
                    ? `${store.detail.members.length} step run${store.detail.members.length === 1 ? "" : "s"} — click to open`
                    : "No member runs started yet"
                }}</span>
              </div>
              <div v-if="store.detail.members.length" class="table-scroll compact-scroll">
                <RunTable
                  :runs="store.detail.members"
                  :selected-run-id="null"
                  :workflow-names="workflowNames"
                  show-workflow
                  entity-label="Workflow"
                  @select="openMemberRun"
                />
              </div>
              <EmptyState
                v-else
                compact
                icon="runs"
                title="No member runs"
                description="Entry members start when the pipeline run begins; more appear as chained links fire."
              />
            </section>

            <section class="grid gap-2 border-t border-border-subtle pt-3">
              <div class="flex items-baseline justify-between gap-2">
                <h2 class="m-0 text-base font-semibold text-fg">Member Attempts</h2>
              </div>
              <div v-if="store.detail.attempts.length" class="grid gap-2">
                <details
                  v-for="attempt in store.detail.attempts"
                  :key="attempt.id"
                  class="rounded-md border border-border-subtle bg-surface-subtle px-3 py-2"
                >
                  <summary class="flex cursor-pointer items-center gap-2 text-sm text-fg">
                    <strong>{{ attempt.member_key }}</strong>
                    <span>#{{ attempt.attempt }}</span>
                    <StatusBadge :status="attempt.status" />
                    <span class="ml-auto text-xs text-fg-muted">{{
                      formatDate(attempt.finished_at ?? attempt.started_at)
                    }}</span>
                  </summary>
                  <p v-if="attempt.message" class="error mb-2 mt-2 text-xs">
                    {{ attempt.message }}
                  </p>
                  <pre class="max-h-56 overflow-auto rounded bg-surface p-2 text-xs">{{
                    JSON.stringify(attempt.result, null, 2)
                  }}</pre>
                  <div class="mt-2 flex gap-2">
                    <button
                      v-if="isRetryableAttempt(attempt)"
                      class="btn btn-primary btn-sm"
                      :disabled="retrying === attempt.id"
                      @click.prevent="retryAttempt(attempt)"
                    >
                      <Icon name="refresh" />
                      <span>Retry member</span>
                    </button>
                    <button
                      v-if="canForceRetryAttempt(attempt)"
                      class="btn btn-danger btn-sm"
                      :disabled="!managedReason.trim() || retrying === attempt.id"
                      @click.prevent="forceRetryAttempt(attempt)"
                    >
                      <Icon name="refresh" />
                      <span>Force retry member</span>
                    </button>
                    <button
                      v-if="attempt.workflow_run_id"
                      class="btn btn-sm"
                      @click.prevent="openMemberRunById(attempt.workflow_run_id)"
                    >
                      <span>Open workflow run</span>
                    </button>
                  </div>
                </details>
              </div>
              <EmptyState v-else compact icon="runs" title="No attempts recorded" />
            </section>
          </div>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import EmptyState from "../components/shared/EmptyState.vue";
import BulkActionBar, { type BulkAction } from "../components/shared/BulkActionBar.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import BrokerMessageLog from "../components/shared/BrokerMessageLog.vue";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";
import RunTable from "../components/shared/RunTable.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useBulkSelection } from "../composables/useBulkSelection";
import { usePipelineRunsStore } from "../../ui/adapters/pinia/pipeline-runs";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOrchestrationsStore } from "../../ui/adapters/pinia/orchestrations";
import { useAuthStore } from "../../ui/adapters/pinia/auth";
import type {
  OrchestrationBinding,
  PipelineMemberAttempt,
  PipelineRunDetail,
  RunSummary,
} from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";
import { countActiveRuns, isActiveRunStatus } from "../../core/utils/status";
import { describeBulkResult } from "../../core/utils/bulk";

const store = usePipelineRunsStore();
const workflows = useWorkflowsStore();
const app = useAppStore();
const orchestrations = useOrchestrationsStore();
const auth = useAuthStore();
const selectedPipelineId = ref("");
const starting = ref(false);
const resolving = ref(false);
const retrying = ref<string | null>(null);
const runControlBusy = ref(false);
const managedIntentBusy = ref(false);
const managedReason = ref("");
const isPlatformAdmin = computed(() => auth.user?.platform_role === "admin");

const managedBindingId = computed(() => store.detail?.run.orchestration_binding_id ?? null);
const managedBinding = computed<OrchestrationBinding | null>(() => {
  const selected = orchestrations.currentBinding();
  return selected?.id === managedBindingId.value ? selected : null;
});
const managedControls = computed(() =>
  Object.entries(managedBinding.value?.policy.intents ?? {})
    .filter(([, policy]) =>
      ["terminate", "suspend", "resume", "supersede", "signal"].includes(policy.effect),
    )
    .sort(([, left], [, right]) => right.priority - left.priority)
    .map(([name, policy]) => ({ name, effect: policy.effect })),
);

// a member with the `inquire` failure mode paused the run; see PipelineDefaults.default_failure_mode
// Recorded on `state.pending_inquiry` while status is
// `approval_required` (reducer::pause_pipeline_run_for_inquiry).
interface PendingInquiry {
  member_run_id: string;
  workflow_id: string;
  status: string;
  raised_at: string;
}

const pendingInquiry = computed<PendingInquiry | null>(() => {
  const run = store.detail?.run;

  if (run?.status !== "approval_required") {
    return null;
  }

  const pending = run.state.pending_inquiry as PendingInquiry | undefined;
  return pending ?? null;
});

function memberName(workflowId: string): string {
  return workflows.workflows.find((wf) => wf.id === workflowId)?.name ?? workflowId;
}

async function resolveInquiry(decision: "continue" | "abort"): Promise<void> {
  const run = store.detail?.run;

  if (!run || resolving.value) {
    return;
  }

  resolving.value = true;

  try {
    await store.resolveRun(run.id, decision);
  } catch (err) {
    app.setError(err instanceof Error ? err.message : String(err));
  } finally {
    resolving.value = false;
  }
}

// adapt each pipeline run to the shared RunSummary shape so the same RunTable renders both families;
// the pipeline id fills the entity column (labeled "Pipeline") the way workflow_id fills it for runs.
const runRows = computed<RunSummary[]>(() =>
  store.runs.map((run) => ({
    id: run.id,
    workflow_id: run.pipeline_id,
    status: run.status,
    trigger_source_kind: run.trigger_source_kind,
    created_at: run.created_at,
    started_at: run.started_at,
    finished_at: run.finished_at,
  })),
);

const selection = useBulkSelection(runRows, (run) => run.id);
const bulkBusy = ref("");
const bulkActions = computed<BulkAction[]>(() => [
  { key: "delete", label: "Delete", icon: "trash", variant: "danger" },
]);

const pipelineNames = computed(() =>
  Object.fromEntries(
    store.pipelines.flatMap((pipeline) =>
      pipeline.id ? ([[pipeline.id, pipeline.name]] as const) : [],
    ),
  ),
);

const workflowNames = computed(() =>
  Object.fromEntries(
    workflows.workflows.flatMap((workflow) =>
      workflow.id ? ([[workflow.id, workflow.name]] as const) : [],
    ),
  ),
);

const activeRunCount = computed(() => countActiveRuns(store.runs));
const selectedRunLabel = computed(() => (store.selectedRunId ? `#${store.selectedRunId}` : "None"));

function pipelineName(pipelineId: string): string {
  return store.pipelines.find((pipeline) => pipeline.id === pipelineId)?.name ?? pipelineId;
}

function onSelectRun(run: RunSummary): void {
  void store.selectRun(run.id);
}

// click-through: open a pipeline member step in the Runs monitor, loading its workflow run detail.
function openMemberRun(run: RunSummary): void {
  void workflows.selectWorkflowRun(run);
  app.activeTab = "Runs";
}

function openMemberRunById(workflowRunId: string): void {
  // Break Vue's recursively-unwrapped Pinia proxy type before traversing the nested run array.
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-assertion
  const detail = store.detail as unknown as PipelineRunDetail | null;
  const run = detail?.members.find((member) => member.id === workflowRunId);

  if (run) {
    openMemberRun(run);
  }
}

function isRetryableAttempt(attempt: PipelineMemberAttempt): boolean {
  if (managedBindingId.value) {
    return false;
  }

  if (!store.detail || !["failed", "timed_out"].includes(attempt.status)) {
    return false;
  }

  return !store.detail.attempts.some(
    (candidate) =>
      candidate.member_key === attempt.member_key && candidate.attempt > attempt.attempt,
  );
}

function canForceRetryAttempt(attempt: PipelineMemberAttempt): boolean {
  return Boolean(
    managedBindingId.value &&
    isPlatformAdmin.value &&
    ["failed", "timed_out"].includes(attempt.status),
  );
}

async function openOrchestration(): Promise<void> {
  if (!managedBindingId.value) {
    return;
  }

  await orchestrations.select(managedBindingId.value);
  app.activeTab = "Orchestrations";
}

async function dispatchManagedIntent(intent: string): Promise<void> {
  if (!managedBindingId.value || !managedReason.value.trim() || managedIntentBusy.value) {
    return;
  }

  managedIntentBusy.value = true;

  try {
    if (orchestrations.selectedId !== managedBindingId.value) {
      await orchestrations.select(managedBindingId.value);
    }

    await orchestrations.dispatch(intent, managedReason.value.trim());
    managedReason.value = "";
    const pipelineRunId = store.detail?.run.id;

    if (pipelineRunId) {
      await store.selectRun(pipelineRunId);
    }
  } catch (err) {
    app.setError(err instanceof Error ? err.message : String(err));
  } finally {
    managedIntentBusy.value = false;
  }
}

async function retryAttempt(attempt: PipelineMemberAttempt): Promise<void> {
  if (!store.detail || retrying.value) {
    return;
  }

  retrying.value = attempt.id;

  try {
    await store.retryMember(store.detail.run.id, attempt.member_key);
  } catch (err) {
    app.setError(err instanceof Error ? err.message : String(err));
  } finally {
    retrying.value = null;
  }
}

async function forceRetryAttempt(attempt: PipelineMemberAttempt): Promise<void> {
  const run = store.detail?.run;
  const reason = managedReason.value.trim();

  if (!run || !reason || retrying.value || !isPlatformAdmin.value) {
    return;
  }

  if (
    !window.confirm(`Force retry member '${attempt.member_key}' outside orchestration control?`)
  ) {
    return;
  }

  retrying.value = attempt.id;

  try {
    await store.retryMember(
      run.id,
      attempt.member_key,
      {},
      {
        reason,
        idempotencyKey: crypto.randomUUID(),
      },
    );
    managedReason.value = "";
  } catch (err) {
    app.setError(err instanceof Error ? err.message : String(err));
  } finally {
    retrying.value = null;
  }
}

async function forceManagedControl(action: "cancel" | "pause" | "resume"): Promise<void> {
  const run = store.detail?.run;
  const reason = managedReason.value.trim();

  if (!run || !reason || !isPlatformAdmin.value || runControlBusy.value) {
    return;
  }

  if (!window.confirm(`Force ${action} this managed pipeline run outside orchestration control?`)) {
    return;
  }

  const override = { reason, idempotencyKey: crypto.randomUUID() };
  await runControl(async () => {
    if (action === "cancel") {
      await store.cancelRun(run.id, override);
    } else if (action === "pause") {
      await store.pauseRun(run.id, override);
    } else {
      await store.resumeRun(run.id, override);
    }

    managedReason.value = "";
  });
}

async function startRun(): Promise<void> {
  if (!selectedPipelineId.value) {
    return;
  }

  starting.value = true;

  try {
    await store.startRun(selectedPipelineId.value);
  } finally {
    starting.value = false;
  }
}

async function cancelRun(pipelineRunId: string): Promise<void> {
  await runControl(() => store.cancelRun(pipelineRunId));
}

async function pauseRun(pipelineRunId: string): Promise<void> {
  await runControl(() => store.pauseRun(pipelineRunId));
}

async function resumeRun(pipelineRunId: string): Promise<void> {
  await runControl(() => store.resumeRun(pipelineRunId));
}

async function runControl(action: () => Promise<void>): Promise<void> {
  if (runControlBusy.value) {
    return;
  }

  runControlBusy.value = true;

  try {
    await action();
  } catch (err) {
    app.setError(err instanceof Error ? err.message : String(err));
  } finally {
    runControlBusy.value = false;
  }
}

async function deleteRun(pipelineRunId: string): Promise<void> {
  if (!window.confirm("Permanently delete this pipeline run and all member workflow history?")) {
    return;
  }

  await store.deleteRun(pipelineRunId);
}

async function deletePipelineRunFromList(run: RunSummary): Promise<void> {
  await deleteRun(run.id);
}

async function runBulkAction(key: string): Promise<void> {
  const selected = selection.selectedRows.value;

  if (!selected.length || bulkBusy.value || key !== "delete") {
    return;
  }

  if (
    !window.confirm(
      `Permanently delete ${String(selected.length)} pipeline run${selected.length === 1 ? "" : "s"} and all member workflow history?\n\nThis cannot be undone.`,
    )
  ) {
    return;
  }

  bulkBusy.value = key;

  try {
    const result = await app.runOperation(`Deleting ${String(selected.length)} pipeline runs`, () =>
      store.deleteRuns(selected.map((run) => run.id)),
    );
    const text = describeBulkResult(result, "Deleted", "pipeline run");

    if (!result.failed.length) {
      app.setStatus(text);
    } else {
      const retryable = result.failed.map((failure) => failure.item);
      app.setError(text, {
        label: `Retry ${String(retryable.length)} failed`,
        run: () => {
          void deletePipelineRuns(retryable);
        },
      });
    }

    selection.clear();
  } finally {
    bulkBusy.value = "";
  }
}

async function deletePipelineRuns(pipelineRunIds: readonly string[]): Promise<void> {
  const result = await app.runOperation(
    `Deleting ${String(pipelineRunIds.length)} pipeline runs`,
    () => store.deleteRuns(pipelineRunIds),
  );
  const text = describeBulkResult(result, "Deleted", "pipeline run");

  if (!result.failed.length) {
    app.setStatus(text);
    return;
  }

  const retryable = result.failed.map((failure) => failure.item);
  app.setError(text, {
    label: `Retry ${String(retryable.length)} failed`,
    run: () => {
      void deletePipelineRuns(retryable);
    },
  });
}

onMounted(() => {
  if (!workflows.workflows.length) {
    void workflows.refreshWorkflows();
  }

  void store.refresh();
});

watch(
  managedBindingId,
  (bindingId) => {
    managedReason.value = "";

    if (bindingId) {
      void orchestrations.select(bindingId);
    }
  },
  { immediate: true },
);
</script>
