<template>
  <section ref="devPane" class="pane dev-pane" tabindex="-1" @keydown="onKeydown">
    <div class="dev-layout">
      <section class="panel dev-panel">
        <div class="panel-toolbar">
          <div class="dev-toolbar-copy">
            <h2>Dev Pack</h2>
            <p>Inspect, edit, apply, and run a local pack without leaving the desktop client.</p>
          </div>
          <div class="actions">
            <button class="btn" :disabled="busy || !packPath.trim()" :title="`Inspect (${modKeyLabel}I)`" @click="inspectPackNow">
              <Icon name="refresh" />
              <span>Inspect</span>
            </button>
            <button class="btn btn-primary" :disabled="busy || !packPath.trim()" :title="`Apply (⇧${modKeyLabel}↵)`" @click="applyPack">
              <Icon name="upload" />
              <span>Apply</span>
            </button>
          </div>
        </div>

        <div class="form-grid dev-form-grid">
          <label>
            <span>Pack path</span>
            <input
              v-model="packPath"
              list="dev-pack-paths"
              placeholder="packs/sdlc/sdlc.wdlp"
              @keydown.enter.prevent="inspectPackNow"
            />
            <datalist id="dev-pack-paths">
              <option v-for="path in recentPacks" :key="path" :value="path" />
            </datalist>
          </label>
          <label>
            <span>Run after apply</span>
            <select v-model="runWorkflowRef">
              <option value="">None</option>
              <option v-for="workflow in availableWorkflows" :key="workflow.id ?? workflow.name" :value="workflow.id ?? workflow.name">
                {{ workflow.name }} v{{ workflow.version }}
              </option>
            </select>
          </label>
          <label>
            <span>Run input</span>
            <RunInputForm
              ref="runInputFormRef"
              v-model="runInputValue"
              :input-type="runWorkflowInputType"
              :storage-key="runWorkflowKey"
            />
          </label>
          <div class="dev-options">
            <label class="check-row">
              <input v-model="skipSettings" type="checkbox" />
              <span>Skip settings</span>
            </label>
            <label class="check-row">
              <input v-model="debugRun" type="checkbox" />
              <span>Debug run</span>
            </label>
            <label class="check-row">
              <input v-model="autoInspect" type="checkbox" />
              <span>Watch files</span>
            </label>
            <label class="check-row">
              <input v-model="autoApply" type="checkbox" />
              <span>Apply on change</span>
            </label>
            <label class="check-row">
              <input v-model="autoSave" type="checkbox" />
              <span>Auto-save edits</span>
            </label>
          </div>
        </div>

        <div class="dev-status-row">
          <StatusBadge :status="statusBadge" />
          <span>{{ statusText }}</span>
        </div>
        <div class="dev-shortcuts">{{ modKeyLabel }}S save · {{ modKeyLabel }}I inspect · {{ modKeyLabel }}↵ run · ⇧{{ modKeyLabel }}↵ apply</div>
        <div v-if="errorText" class="dev-error">{{ errorText }}</div>

        <div class="dev-metrics">
          <div>
            <span>Workflows</span>
            <strong>{{ inspectResult?.workflows.length ?? 0 }}</strong>
          </div>
          <div>
            <span>Triggers</span>
            <strong>{{ inspectResult?.triggers.length ?? 0 }}</strong>
          </div>
          <div>
            <span>Settings</span>
            <strong>{{ inspectResult?.settings_count ?? 0 }}</strong>
          </div>
          <div>
            <span>Files</span>
            <strong>{{ watchedFiles.length }}</strong>
          </div>
        </div>

        <div class="dev-section-header">
          <h3>Pending Changes</h3>
          <span>vs current server state</span>
        </div>
        <PackDiff
          class="dev-pack-diff"
          :pack="inspectResult"
          :existing-workflows="workflows.workflows"
          :existing-settings="secrets.secrets"
        />

        <div class="dev-section-header">
          <h3>Watched Files</h3>
          <span>{{ lastInspectText }}</span>
        </div>
        <div class="dev-file-list">
          <button
            v-for="file in watchedFiles"
            :key="file.path"
            :class="{ selected: selectedFilePath === file.path }"
            @click="selectFile(file.path)"
          >
            <span class="dev-file-kind">{{ file.kind }}</span>
            <span class="dev-file-path">{{ relativePath(file.path) }}</span>
            <span class="dev-file-meta">{{ fileMeta(file) }}</span>
          </button>
        </div>
      </section>

      <section class="panel dev-editor-panel">
        <div class="panel-toolbar">
          <div class="dev-toolbar-copy">
            <h2>{{ selectedFilePath ? relativePath(selectedFilePath) : "Source" }}</h2>
            <p>{{ selectedFilePath ? "Live source editing for the selected pack file." : "Select a watched file to inspect its source." }}</p>
          </div>
          <div class="actions">
            <button class="btn" :disabled="!canSaveSource || saving" @click="saveSelectedSource">
              <Icon name="save" />
              <span>Save</span>
            </button>
            <button class="btn" :disabled="!selectedFilePath || busy" @click="reloadSelectedSource">
              <Icon name="refresh" />
              <span>Reload</span>
            </button>
          </div>
        </div>

        <WdlEditor
          v-if="selectedIsWdl"
          v-model="sourceText"
          class="dev-wdl-editor"
          title="WDL"
          :providers="providers.providers"
          :settings="secrets.secrets"
          :source-path="selectedFilePath"
        />
        <JsonEditor
          v-else-if="selectedIsJson"
          v-model="sourceText"
          class="dev-json-editor"
          title="JSON"
        />
        <textarea
          v-else
          v-model="sourceText"
          class="dev-plain-source"
          spellcheck="false"
          :readonly="!selectedFilePath"
        ></textarea>
      </section>

      <section class="panel dev-run-panel">
        <div class="panel-toolbar">
          <div class="dev-toolbar-copy">
            <h2>Latest Run</h2>
            <p>{{ latestRunId ? "Track the active or most recent run created from this panel." : "Run a selected workflow and inspect its latest execution here." }}</p>
          </div>
          <div class="actions">
            <button class="btn btn-primary" :disabled="!canRun" :title="`Run (${modKeyLabel}↵)`" @click="runSelectedWorkflow">
              <Icon name="play" />
              <span>{{ latestRunId ? "Re-run" : "Run" }}</span>
            </button>
            <button v-if="runInFlight" class="btn btn-danger" title="Cancel this run" @click="cancelRun">
              <Icon name="stop" />
              <span>Cancel</span>
            </button>
            <button class="btn" :disabled="!latestRunId || busy" @click="refreshLatestRun">
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </div>
        </div>
        <div v-if="recentRunIds.length > 1" class="dev-recent-runs">
          <span class="dev-recent-label">Recent:</span>
          <button
            v-for="id in recentRunIds"
            :key="id"
            class="dev-run-pill"
            :class="{ active: id === latestRunId }"
            @click="viewRun(id)"
          >#{{ id }}</button>
        </div>
        <template v-if="latestRunDetail">
          <div class="dev-run-summary">
            <div>
              <span>Run</span>
              <strong>#{{ latestRunDetail.run.id }}</strong>
            </div>
            <div>
              <span>Status</span>
              <StatusBadge :status="latestRunDetail.run.status" />
            </div>
            <div>
              <span>Active</span>
              <strong>{{ latestRunDetail.run.active_node_id ?? "-" }}</strong>
            </div>
            <div>
              <span>Steps</span>
              <strong class="dev-run-counts">
                <span class="ok">{{ runNodeCounts.ok }}✓</span>
                <span v-if="runNodeCounts.failed" class="failed">{{ runNodeCounts.failed }}✕</span>
                <span v-if="runNodeCounts.running" class="running">{{ runNodeCounts.running }}⟳</span>
              </strong>
            </div>
          </div>
          <RunTimeline
            class="dev-run-timeline"
            :detail="latestRunDetail"
            :selected-node-id="selectedRunNodeId"
            auto-expand-failed
            filterable
            @select="selectRunNode"
          >
            <template #node-actions="{ node }">
              <RunNodeActions
                :node="node"
                :run="latestRunDetail.run"
                :busy="busy"
                @action="onRunNodeAction"
              />
            </template>
          </RunTimeline>
        </template>
        <div v-else class="empty-state">No run started from this panel.</div>
      </section>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  applyDevPack,
  cancelWorkflowRun,
  createWorkflowRun,
  fetchWorkflowRun,
  inspectDevPack,
  readDevPackFile,
  replayWorkflowRun,
  writeDevPackFile
} from "../api/commandCenterApi";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import PackDiff from "../components/shared/PackDiff.vue";
import RunInputForm from "../components/shared/RunInputForm.vue";
import RunNodeActions, { type RunNodeActionType } from "../components/shared/RunNodeActions.vue";
import RunTimeline from "../components/shared/RunTimeline.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import WdlEditor from "../components/shared/WdlEditor.vue";
import { useAppStore } from "../stores/app";
import { useProvidersStore } from "../stores/providers";
import { useSecretsStore } from "../stores/secrets";
import { useWorkflowsStore } from "../stores/workflows";
import type { DevPackFile, DevPackInspectResult, RuninatorType, WorkflowNodeRun, WorkflowRunDetail } from "../types/models";

const DEFAULT_PACK_PATH = "packs/sdlc/sdlc.wdlp";
const TERMINAL_STATUSES = new Set(["succeeded", "failed", "canceled", "timed_out"]);

const app = useAppStore();
const workflows = useWorkflowsStore();
const providers = useProvidersStore();
const secrets = useSecretsStore();

const OPTIONS_STORAGE_KEY = "runinator.devPack.options";
const savedOptions = loadDevOptions();
const modKeyLabel = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

const packPath = ref(window.localStorage.getItem("runinator.devPack.path") || DEFAULT_PACK_PATH);
const skipSettings = ref(Boolean(savedOptions.skipSettings));
const autoInspect = ref(savedOptions.autoInspect ?? true);
const autoApply = ref(Boolean(savedOptions.autoApply));
const autoSave = ref(Boolean(savedOptions.autoSave));
const debugRun = ref(Boolean(savedOptions.debugRun));
const runWorkflowRef = ref(String(savedOptions.runWorkflowRef ?? ""));
const recentRunIds = ref<string[]>([]);
const recentPacks = ref<string[]>(loadRecentPacks());
const runInputValue = ref<unknown>({});
const runInputFormRef = ref<InstanceType<typeof RunInputForm> | null>(null);
const inspectResult = ref<DevPackInspectResult | null>(null);
const selectedFilePath = ref(window.localStorage.getItem("runinator.devPack.file") || "");
const sourceText = ref("");
const savedSourceText = ref("");
const latestRunId = ref<string | null>(null);
const latestRunDetail = ref<WorkflowRunDetail | null>(null);
const selectedRunNodeId = ref<string | null>(null);
const errorText = ref("");
const statusText = ref("Ready.");
const lastInspectAt = ref<Date | null>(null);
const busy = ref(false);
const saving = ref(false);
const devPane = ref<HTMLElement | null>(null);
let inspectTimer = 0;
let runTimer = 0;
let lastFingerprint = "";

const watchedFiles = computed(() => inspectResult.value?.files ?? []);
const availableWorkflows = computed(() => inspectResult.value?.workflows ?? workflows.workflows);
const selectedIsWdl = computed(() => selectedFilePath.value.endsWith(".wdl"));
const selectedIsJson = computed(() => selectedFilePath.value.endsWith(".json"));
const runWorkflowInputType = computed<RuninatorType>(() => resolveRunWorkflow()?.input_type ?? { type: "any" });
const runWorkflowKey = computed(() => String(runWorkflowRef.value || "none"));
const canSaveSource = computed(() => (selectedIsWdl.value || selectedIsJson.value) && sourceText.value !== savedSourceText.value);
const canRun = computed(() => Boolean(runWorkflowRef.value) && !busy.value);
const runInFlight = computed(() => {
  const status = latestRunDetail.value?.run.status;
  return Boolean(status) && !TERMINAL_STATUSES.has(status ?? "");
});
const statusBadge = computed(() => (errorText.value ? "failed" : busy.value || saving.value ? "running" : "succeeded"));
const lastInspectText = computed(() => (lastInspectAt.value ? `Last inspect ${lastInspectAt.value.toLocaleTimeString()}` : "Not inspected"));
const runNodeCounts = computed(() => {
  const counts = { ok: 0, failed: 0, running: 0 };
  for (const node of latestRunDetail.value?.nodes ?? []) {
    if (node.status === "succeeded") counts.ok += 1;
    else if (node.status === "failed" || node.status === "timed_out") counts.failed += 1;
    else if (["running", "waiting", "queued", "retrying"].includes(node.status)) counts.running += 1;
  }
  return counts;
});

onMounted(async () => {
  await providers.fetchProviders().catch(() => {});
  if (secrets.secrets.length === 0) await secrets.refreshSecrets().catch(() => {});
  await workflows.refreshWorkflows().catch(() => {});
  await inspectPack();
  inspectTimer = window.setInterval(() => {
    if (autoInspect.value && packPath.value.trim() && !busy.value) {
      void inspectPack({ quiet: true, applyOnChange: autoApply.value });
    }
  }, 1500);
  // focus the pane so its scoped keydown shortcuts work without first clicking inside.
  devPane.value?.focus();
});

onBeforeUnmount(() => {
  window.clearInterval(inspectTimer);
  window.clearInterval(runTimer);
  window.clearTimeout(autoSaveTimer);
  document.title = defaultDocumentTitle;
});

watch(packPath, (value) => {
  window.localStorage.setItem("runinator.devPack.path", value);
});

// remember the run loop's toggles and target across reloads.
watch([skipSettings, autoInspect, autoApply, autoSave, debugRun, runWorkflowRef], () => {
  window.localStorage.setItem(
    OPTIONS_STORAGE_KEY,
    JSON.stringify({
      skipSettings: skipSettings.value,
      autoInspect: autoInspect.value,
      autoApply: autoApply.value,
      autoSave: autoSave.value,
      debugRun: debugRun.value,
      runWorkflowRef: runWorkflowRef.value
    })
  );
});

watch(selectedFilePath, (value) => {
  window.localStorage.setItem("runinator.devPack.file", value);
});

// auto-save the edited wdl to disk (debounced) so the watch/apply loop sees in-app edits.
let autoSaveTimer = 0;
watch(sourceText, () => {
  if (!autoSave.value) return;
  window.clearTimeout(autoSaveTimer);
  autoSaveTimer = window.setTimeout(() => {
    if (autoSave.value && canSaveSource.value && !saving.value && !busy.value) void saveSelectedSource();
  }, 800);
});

function loadDevOptions(): Record<string, any> {
  try {
    return JSON.parse(window.localStorage.getItem(OPTIONS_STORAGE_KEY) || "{}");
  } catch {
    return {};
  }
}

// edit-loop keyboard shortcuts: save, inspect, run, and apply.
function onKeydown(event: KeyboardEvent) {
  if (!event.metaKey && !event.ctrlKey) return;
  const key = event.key.toLowerCase();
  if (key === "s") {
    event.preventDefault();
    if (canSaveSource.value && !saving.value) void saveSelectedSource();
  } else if (key === "i") {
    event.preventDefault();
    if (!busy.value && packPath.value.trim()) inspectPackNow();
  } else if (key === "enter") {
    event.preventDefault();
    if (event.shiftKey) {
      if (!busy.value && packPath.value.trim()) void applyPack();
    } else if (canRun.value) {
      void runSelectedWorkflow();
    }
  }
}

function rememberRun(id: string) {
  recentRunIds.value = [id, ...recentRunIds.value.filter((existing) => existing !== id)].slice(0, 8);
}

async function viewRun(id: string) {
  if (id === latestRunId.value && latestRunDetail.value) return;
  latestRunId.value = id;
  await refreshLatestRun();
  watchLatestRun();
}

async function cancelRun() {
  if (!latestRunId.value || !runInFlight.value) return;
  try {
    await cancelWorkflowRun(latestRunId.value);
    statusText.value = `Canceled run #${latestRunId.value}.`;
    await refreshLatestRun();
  } catch (err) {
    errorText.value = String(err);
  }
}

function loadRecentPacks(): string[] {
  try {
    return JSON.parse(window.localStorage.getItem("runinator.devPack.recentPaths") || "[]");
  } catch {
    return [];
  }
}

function rememberPack(path: string) {
  recentPacks.value = [path, ...recentPacks.value.filter((existing) => existing !== path)].slice(0, 8);
  window.localStorage.setItem("runinator.devPack.recentPaths", JSON.stringify(recentPacks.value));
}

// reflect the run status in the tab title so a completed run is noticeable from another tab.
const defaultDocumentTitle = document.title;
watch(
  () => [latestRunId.value, latestRunDetail.value?.run.status] as const,
  ([id, status]) => {
    if (!id || !status) {
      document.title = defaultDocumentTitle;
      return;
    }
    const icon = status === "succeeded" ? "✓" : status === "failed" || status === "timed_out" ? "✕" : "▶";
    document.title = `${icon} #${id} ${status} · Runinator`;
  }
);

async function inspectPack(options: { quiet?: boolean; applyOnChange?: boolean } = {}) {
  const path = packPath.value.trim();
  if (!path) return;
  if (!options.quiet) {
    errorText.value = "";
    statusText.value = "Inspecting pack...";
  }
  busy.value = true;
  try {
    const result = await inspectDevPack(path, skipSettings.value);
    const previousFingerprint = lastFingerprint;
    inspectResult.value = result;
    rememberPack(path);
    lastInspectAt.value = new Date();
    lastFingerprint = fingerprint(result.files);
    if (!selectedFilePath.value || !result.files.some((file) => file.path === selectedFilePath.value)) {
      const firstWdl = result.files.find((file) => file.kind === "workflow") ?? result.files[0];
      if (firstWdl) await selectFile(firstWdl.path);
    } else if (previousFingerprint && previousFingerprint !== lastFingerprint) {
      await reloadSelectedSource();
    }
    statusText.value = `Pack ready: ${result.workflows.length} workflow${result.workflows.length === 1 ? "" : "s"}.`;
    if (options.applyOnChange && previousFingerprint && previousFingerprint !== lastFingerprint) {
      await applyPack();
    }
  } catch (err) {
    errorText.value = String(err);
    statusText.value = "Inspect failed.";
  } finally {
    busy.value = false;
  }
}

function inspectPackNow() {
  void inspectPack();
}

async function applyPack() {
  const path = packPath.value.trim();
  if (!path) return;
  errorText.value = "";
  statusText.value = "Applying pack...";
  busy.value = true;
  try {
    const result = await applyDevPack(path, skipSettings.value);
    await workflows.refreshWorkflows().catch(() => {});
    inspectResult.value = {
      path: result.path,
      files: result.files,
      workflows: result.imported.workflows.workflows,
      triggers: result.imported.workflows.triggers,
      settings_count: result.imported.secrets?.secrets?.length ?? 0,
      // re-inspect repopulates real setting identities; after apply they are already on the server.
      settings: inspectResult.value?.settings ?? []
    };
    lastFingerprint = fingerprint(result.files);
    lastInspectAt.value = new Date();
    statusText.value = `Applied ${result.imported.workflows.workflows.length} workflow${result.imported.workflows.workflows.length === 1 ? "" : "s"}.`;
    if (runWorkflowRef.value) {
      await runSelectedWorkflow();
    }
  } catch (err) {
    errorText.value = String(err);
    statusText.value = "Apply failed.";
  } finally {
    busy.value = false;
  }
}

async function runSelectedWorkflow() {
  const workflow = resolveRunWorkflow();
  if (!workflow?.id) {
    errorText.value = `Workflow not found: ${runWorkflowRef.value}`;
    return;
  }
  const parameters = runInputValue.value ?? {};
  const created = await createWorkflowRun(workflow.id, { debug: debugRun.value, parameters });
  runInputFormRef.value?.persistLast();
  latestRunId.value = created.id;
  rememberRun(created.id);
  statusText.value = `Started workflow run #${created.id}.`;
  await refreshLatestRun();
  watchLatestRun();
}

function resolveRunWorkflow() {
  const value = runWorkflowRef.value;
  const byId = availableWorkflows.value.find((workflow) => workflow.id === value) ?? workflows.workflows.find((workflow) => workflow.id === value);
  if (byId) return byId;
  return availableWorkflows.value.find((workflow) => workflow.name === value) ?? workflows.workflows.find((workflow) => workflow.name === value);
}

async function refreshLatestRun() {
  if (!latestRunId.value) return;
  latestRunDetail.value = await fetchWorkflowRun(latestRunId.value);
}

function selectRunNode(nodeId: string) {
  selectedRunNodeId.value = nodeId;
}

async function onRunNodeAction(payload: { type: RunNodeActionType; node: WorkflowNodeRun }) {
  if (!latestRunDetail.value) return;
  // the dev panel has no canvas, so editor/provider actions are handled by the standalone views.
  if (payload.type !== "replay-run" && payload.type !== "replay-from") return;
  const runId = latestRunDetail.value.run.id;
  busy.value = true;
  errorText.value = "";
  try {
    const options = payload.type === "replay-from" ? { fromStepId: payload.node.node_id } : {};
    const created = await replayWorkflowRun(runId, options);
    latestRunId.value = created.id;
    rememberRun(created.id);
    selectedRunNodeId.value = null;
    statusText.value = `Replayed run #${runId} as #${created.id}.`;
    await refreshLatestRun();
    watchLatestRun();
  } catch (err) {
    errorText.value = String(err);
  } finally {
    busy.value = false;
  }
}

function watchLatestRun() {
  window.clearInterval(runTimer);
  runTimer = window.setInterval(async () => {
    if (!latestRunId.value) return;
    await refreshLatestRun().catch((err) => {
      errorText.value = String(err);
    });
    const status = latestRunDetail.value?.run.status;
    if (status && TERMINAL_STATUSES.has(status)) {
      window.clearInterval(runTimer);
    }
  }, 1500);
}

async function selectFile(path: string) {
  selectedFilePath.value = path;
  await reloadSelectedSource();
}

async function reloadSelectedSource() {
  if (!selectedFilePath.value) return;
  try {
    const file = await readDevPackFile(selectedFilePath.value);
    sourceText.value = file.content;
    savedSourceText.value = file.content;
  } catch (err) {
    errorText.value = String(err);
  }
}

async function saveSelectedSource() {
  if (!selectedFilePath.value || !canSaveSource.value) return;
  saving.value = true;
  errorText.value = "";
  try {
    const file = await writeDevPackFile(selectedFilePath.value, sourceText.value);
    sourceText.value = file.content;
    savedSourceText.value = file.content;
    statusText.value = `Saved ${relativePath(file.path)}.`;
    await inspectPack({ quiet: true, applyOnChange: autoApply.value });
  } catch (err) {
    errorText.value = String(err);
  } finally {
    saving.value = false;
  }
}

function fingerprint(files: DevPackFile[]) {
  return files.map((file) => `${file.path}:${file.modified_at ?? ""}:${file.size_bytes ?? ""}`).join("|");
}

function relativePath(path: string) {
  const root = packPath.value.replace(/\/[^/]*$/, "");
  return path.startsWith(root) ? path.slice(root.length + 1) || path : path;
}

function fileMeta(file: DevPackFile) {
  const size = file.size_bytes == null ? "-" : `${file.size_bytes}b`;
  const time = file.modified_at ? new Date(file.modified_at).toLocaleTimeString() : "-";
  return `${size} · ${time}`;
}
</script>

<style scoped>
.dev-pane {
  overflow: auto;
}

.dev-toolbar-copy {
  display: grid;
  gap: 4px;
}

.dev-toolbar-copy p {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.dev-layout {
  display: grid;
  min-height: 100%;
  gap: 12px;
  grid-template-columns: minmax(320px, 380px) minmax(0, 1fr);
  grid-template-rows: minmax(520px, 1fr) minmax(220px, 34vh);
}

.dev-panel {
  grid-row: 1 / span 2;
}

.dev-editor-panel,
.dev-run-panel,
.dev-panel {
  min-height: 0;
}

.dev-form-grid {
  grid-template-columns: 1fr;
  gap: 12px;
  padding: 2px 0;
}

.dev-form-grid label {
  padding: 10px 12px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.dev-pack-diff {
  margin: 4px 0 6px;
}

.dev-shortcuts {
  margin-top: 2px;
  color: var(--text-faint);
  font-size: 11px;
}

.dev-recent-runs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.dev-recent-label {
  color: #66717e;
  font-size: 11px;
}

.dev-run-pill {
  border: 1px solid #c8d1db;
  border-radius: 999px;
  background: #f8fafc;
  color: #344255;
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 9px;
}

.dev-run-pill.active {
  border-color: #2563eb;
  background: #eef5ff;
  color: #1d4ed8;
}

.dev-options {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.check-row {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  min-height: 40px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  color: var(--text);
  font-size: 13px;
  padding: 0 10px;
}

.dev-status-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  color: var(--text);
  font-size: 13px;
  padding: 10px 12px;
}

.dev-error {
  margin-top: 8px;
  border-left: 3px solid var(--danger-solid);
  background: var(--danger-bg);
  color: var(--danger-fg);
  padding: 10px 12px;
  white-space: pre-wrap;
  font-size: 12px;
}

.dev-metrics {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  margin: 12px 0;
}

.dev-metrics div,
.dev-run-summary div {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  padding: 10px;
  background: var(--surface-subtle);
}

.dev-metrics span,
.dev-run-summary span {
  display: block;
  color: var(--text-muted);
  font-size: 11px;
}

.dev-metrics strong,
.dev-run-summary strong {
  color: var(--text);
  font-size: 15px;
}

.dev-section-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  margin: 10px 0 6px;
}

.dev-section-header h3 {
  margin: 0;
  font-size: 13px;
}

.dev-section-header span {
  color: var(--text-muted);
  font-size: 11px;
}

.dev-file-list {
  display: grid;
  gap: 6px;
  overflow: auto;
}

.dev-file-list button {
  display: grid;
  grid-template-columns: 68px minmax(0, 1fr);
  gap: 3px 8px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  color: var(--text);
  padding: 9px 10px;
  text-align: left;
}

.dev-file-list button:hover,
.dev-file-list button.selected {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.dev-file-kind {
  color: var(--text-subtle);
  font-size: 11px;
  text-transform: uppercase;
}

.dev-file-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}

.dev-file-meta {
  grid-column: 2;
  color: var(--text-muted);
  font-size: 11px;
}

.dev-editor-panel,
.dev-run-panel {
  display: flex;
  flex-direction: column;
}

.dev-wdl-editor {
  min-height: 0;
}

.dev-json-editor {
  min-height: 0;
}

.dev-plain-source {
  flex: 1 1 auto;
  min-height: 0;
  border: 0;
  border-top: 1px solid var(--border-subtle);
  padding: 12px 0 0;
  resize: none;
  background: transparent;
  font: 12px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.dev-run-summary {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(2, minmax(0, 1fr)) minmax(0, 1.2fr) minmax(0, 1fr);
  margin-bottom: 10px;
}

.dev-run-counts {
  display: inline-flex;
  gap: 8px;
  font-variant-numeric: tabular-nums;
}
.dev-run-counts .ok {
  color: var(--success-fg);
}
.dev-run-counts .failed {
  color: var(--danger-fg);
}
.dev-run-counts .running {
  color: var(--accent-text);
}

.dev-run-timeline {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  border-top: 1px solid var(--border-subtle);
  padding-top: 8px;
}

.empty-state {
  color: var(--text-muted);
  padding: 14px 0;
}

@media (max-width: 980px) {
  .dev-layout {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(460px, 1fr) minmax(220px, auto);
  }

  .dev-panel {
    grid-row: auto;
  }
}
</style>
