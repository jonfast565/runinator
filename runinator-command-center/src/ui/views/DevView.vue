<template src="./dev-view.template.html"></template>

<script setup lang="ts">
/* eslint-disable @typescript-eslint/no-unused-vars -- external Vue templates are not visible to ESLint. */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { devPackService } from "../../core/services";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import PackDiff from "../components/shared/PackDiff.vue";
import RunInputForm from "../components/shared/RunInputForm.vue";
import RunNodeActions, { type RunNodeActionType } from "../components/shared/RunNodeActions.vue";
import RunTimeline from "../components/shared/RunTimeline.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import WdlEditor from "../components/shared/WdlEditor.vue";
import { useProvidersStore } from "../../ui/adapters/pinia/providers";
import { useSecretsStore } from "../../ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import { useOperationLoading } from "../composables/useOperationLoading";
import { displayValue } from "../../core/utils/values";
import type { DevPackInspectResult, RuninatorType, WorkflowNodeRun, WorkflowRunDetail } from "../../core/domain/models";
import { workflowInputType } from "../../core/domain/models";
import {
  DEV_OPTIONS_STORAGE_KEY,
  fileMeta,
  fingerprint,
  loadDevOptions,
  loadRecentPacks,
  relativePackPath,
} from "./dev-view-files";

const DEFAULT_PACK_PATH = "packs/sdlc/sdlc.wdlm";
const TERMINAL_STATUSES = new Set(["succeeded", "failed", "canceled", "timed_out"]);

const workflows = useWorkflowsStore();
const providers = useProvidersStore();
const secrets = useSecretsStore();
const { isLoading: inspecting } = useOperationLoading("Inspecting dev pack");
const { isLoading: applying } = useOperationLoading("Applying dev pack");
const { isLoading: readingFile } = useOperationLoading("Reading dev pack file");
const { isLoading: writingFile } = useOperationLoading("Writing dev pack file");
const { isLoading: startingRun } = useOperationLoading("Starting workflow run");
const { isLoading: loadingRun } = useOperationLoading("Loading workflow run");
const { isLoading: cancelingRun } = useOperationLoading("Canceling workflow run");
const { isLoading: replayingRun } = useOperationLoading("Replaying workflow run");

const savedOptions = loadDevOptions();
const modKeyLabel = /mac/i.test(navigator.userAgent) ? "⌘" : "Ctrl+";

const packPath = ref(window.localStorage.getItem("runinator.devPack.path") ?? DEFAULT_PACK_PATH);
const skipSettings = ref(Boolean(savedOptions.skipSettings));
const autoInspect = ref(
  typeof savedOptions.autoInspect === "boolean" ? savedOptions.autoInspect : true,
);
const autoApply = ref(Boolean(savedOptions.autoApply));
const autoSave = ref(Boolean(savedOptions.autoSave));
const debugRun = ref(Boolean(savedOptions.debugRun));
const runWorkflowRef = ref(displayValue(savedOptions.runWorkflowRef));
const recentRunIds = ref<string[]>([]);
const recentPacks = ref<string[]>(loadRecentPacks());
const runInputValue = ref<unknown>({});
const runInputFormRef = ref<InstanceType<typeof RunInputForm> | null>(null);
const inspectResult = ref<DevPackInspectResult | null>(null);
const selectedFilePath = ref(window.localStorage.getItem("runinator.devPack.file") ?? "");
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
const runWorkflowInputType = computed((): RuninatorType => {
  const workflow = resolveRunWorkflow();
  return workflow ? (workflowInputType(workflow) ?? { type: "any" }) : { type: "any" };
});
const runWorkflowKey = computed(() => runWorkflowRef.value || "none");
const canSaveSource = computed(
  () => (selectedIsWdl.value || selectedIsJson.value) && sourceText.value !== savedSourceText.value,
);
const canRun = computed(
  () => Boolean(runWorkflowRef.value) && !busy.value && !startingRun.value,
);
const runInFlight = computed(() => {
  const status = latestRunDetail.value?.run.status;
  return Boolean(status) && !TERMINAL_STATUSES.has(status ?? "");
});
const statusBadge = computed(() =>
  errorText.value ? "failed" : busy.value || saving.value ? "running" : "succeeded",
);
const lastInspectText = computed(() =>
  lastInspectAt.value
    ? `Last inspect ${lastInspectAt.value.toLocaleTimeString()}`
    : "Not inspected",
);
const runNodeCounts = computed(() => {
  const counts = { ok: 0, failed: 0, running: 0 };

  for (const node of latestRunDetail.value?.nodes ?? []) {
    if (node.status === "succeeded") {
      counts.ok += 1;
    } else if (node.status === "failed" || node.status === "timed_out") {
      counts.failed += 1;
    } else if (["running", "waiting", "queued", "retrying"].includes(node.status)) {
      counts.running += 1;
    }
  }

  return counts;
});

onMounted(async () => {
  await providers.fetchProviders().catch(() => undefined);

  if (secrets.secrets.length === 0) {
    await secrets.refreshSecrets().catch(() => undefined);
  }

  await workflows.refreshWorkflows().catch(() => undefined);
  await inspectPack();
  inspectTimer = window.setInterval(() => {
    if (autoInspect.value && !busy.value) {
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
    DEV_OPTIONS_STORAGE_KEY,
    JSON.stringify({
      skipSettings: skipSettings.value,
      autoInspect: autoInspect.value,
      autoApply: autoApply.value,
      autoSave: autoSave.value,
      debugRun: debugRun.value,
      runWorkflowRef: runWorkflowRef.value,
    }),
  );
});

watch(selectedFilePath, (value) => {
  window.localStorage.setItem("runinator.devPack.file", value);
});

// auto-save the edited wdl to disk (debounced) so the watch/apply loop sees in-app edits.
let autoSaveTimer = 0;
watch(sourceText, () => {
  if (!autoSave.value) {
    return;
  }

  window.clearTimeout(autoSaveTimer);
  autoSaveTimer = window.setTimeout(() => {
    if (autoSave.value && canSaveSource.value && !saving.value && !busy.value) {
      void saveSelectedSource();
    }
  }, 800);
});

// edit-loop keyboard shortcuts: save, inspect, run, and apply.
function onKeydown(event: KeyboardEvent) {
  if (!event.metaKey && !event.ctrlKey) {
    return;
  }

  const key = event.key.toLowerCase();

  if (key === "s") {
    event.preventDefault();

    if (canSaveSource.value && !saving.value) {
      void saveSelectedSource();
    }
  } else if (key === "i") {
    event.preventDefault();

    if (!busy.value && packPath.value.trim()) {
      inspectPackNow();
    }
  } else if (key === "enter") {
    event.preventDefault();

    if (event.shiftKey) {
      if (!busy.value && packPath.value.trim()) {
        void applyPack();
      }
    } else if (canRun.value) {
      void runSelectedWorkflow();
    }
  }
}

function rememberRun(id: string) {
  recentRunIds.value = [id, ...recentRunIds.value.filter((existing) => existing !== id)].slice(
    0,
    8,
  );
}

async function viewRun(id: string) {
  if (id === latestRunId.value && latestRunDetail.value) {
    return;
  }

  latestRunId.value = id;
  await refreshLatestRun();
  watchLatestRun();
}

async function cancelRun() {
  if (!latestRunId.value || !runInFlight.value) {
    return;
  }

  try {
    await devPackService.cancelRun(latestRunId.value);
    statusText.value = `Canceled run #${latestRunId.value}.`;
    await refreshLatestRun();
  } catch (err) {
    errorText.value = String(err);
  }
}

function rememberPack(path: string) {
  recentPacks.value = [path, ...recentPacks.value.filter((existing) => existing !== path)].slice(
    0,
    8,
  );
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

    const icon =
      status === "succeeded" ? "✓" : status === "failed" || status === "timed_out" ? "✕" : "▶";
    document.title = `${icon} #${id} ${status} · Runinator`;
  },
);

async function inspectPack(options: { quiet?: boolean; applyOnChange?: boolean } = {}) {
  const path = packPath.value.trim();

  if (!path) {
    return;
  }

  if (!options.quiet) {
    errorText.value = "";
    statusText.value = "Inspecting pack...";
  }

  busy.value = true;

  try {
    const result = await devPackService.inspect(path, skipSettings.value);
    const previousFingerprint = lastFingerprint;
    inspectResult.value = result;
    rememberPack(path);
    lastInspectAt.value = new Date();
    lastFingerprint = fingerprint(result.files);

    if (
      !selectedFilePath.value ||
      !result.files.some((file) => file.path === selectedFilePath.value)
    ) {
      const firstWdl = result.files.find((file) => file.kind === "workflow") ?? result.files[0];
      await selectFile(firstWdl.path);
    } else if (previousFingerprint && previousFingerprint !== lastFingerprint) {
      await reloadSelectedSource();
    }

    statusText.value = `Pack ready: ${String(result.workflows.length)} workflow${result.workflows.length === 1 ? "" : "s"}.`;

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

  if (!path) {
    return;
  }

  errorText.value = "";
  statusText.value = "Applying pack...";
  busy.value = true;

  try {
    const result = await devPackService.apply(path, skipSettings.value);
    await workflows.refreshWorkflows().catch(() => undefined);
    inspectResult.value = {
      path: result.path,
      files: result.files,
      workflows: result.imported.workflows.workflows,
      triggers: result.imported.workflows.triggers,
      settings_count: result.imported.secrets?.secrets?.length ?? 0,
      // re-inspect repopulates real setting identities; after apply they are already on the server.
      settings: inspectResult.value?.settings ?? [],
    };
    lastFingerprint = fingerprint(result.files);
    lastInspectAt.value = new Date();
    statusText.value = `Applied ${String(result.imported.workflows.workflows.length)} workflow${result.imported.workflows.workflows.length === 1 ? "" : "s"}.`;

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
  const created = await devPackService.createRun(workflow.id, { debug: debugRun.value, parameters });
  runInputFormRef.value?.persistLast();
  latestRunId.value = created.id;
  rememberRun(created.id);
  statusText.value = `Started workflow run #${created.id}.`;
  await refreshLatestRun();
  watchLatestRun();
}

function resolveRunWorkflow() {
  const value = runWorkflowRef.value;
  const byId =
    availableWorkflows.value.find((workflow) => workflow.id === value) ??
    workflows.workflows.find((workflow) => workflow.id === value);

  if (byId) {
    return byId;
  }

  return (
    availableWorkflows.value.find((workflow) => workflow.name === value) ??
    workflows.workflows.find((workflow) => workflow.name === value)
  );
}

async function refreshLatestRun() {
  if (!latestRunId.value) {
    return;
  }

  latestRunDetail.value = await devPackService.fetchRun(latestRunId.value);
}

function selectRunNode(nodeId: string) {
  selectedRunNodeId.value = nodeId;
}

async function onRunNodeAction(payload: { type: RunNodeActionType; node: WorkflowNodeRun }) {
  if (!latestRunDetail.value) {
    return;
  }

  // the dev panel has no canvas, so editor/provider actions are handled by the standalone views.
  if (payload.type !== "replay-run" && payload.type !== "replay-from") {
    return;
  }

  const runId = latestRunDetail.value.run.id;
  busy.value = true;
  errorText.value = "";

  try {
    const options = payload.type === "replay-from" ? { fromStepId: payload.node.node_id } : {};
    const created = await devPackService.replayRun(runId, options);
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
    if (!latestRunId.value) {
      return;
    }

    await refreshLatestRun().catch((err: unknown) => {
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
  if (!selectedFilePath.value) {
    return;
  }

  try {
    const file = await devPackService.readFile(selectedFilePath.value);
    sourceText.value = file.content;
    savedSourceText.value = file.content;
  } catch (err) {
    errorText.value = String(err);
  }
}

async function saveSelectedSource() {
  if (!selectedFilePath.value || !canSaveSource.value) {
    return;
  }

  saving.value = true;
  errorText.value = "";

  try {
    const file = await devPackService.writeFile(selectedFilePath.value, sourceText.value);
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

function relativePath(path: string) {
  return relativePackPath(path, packPath.value);
}
</script>
