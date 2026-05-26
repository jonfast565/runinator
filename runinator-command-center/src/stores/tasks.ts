import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  fetchRunArtifacts,
  fetchRunChunks,
  fetchTaskRuns,
  fetchTasks,
  requestTaskRun,
  saveTask
} from "../api/commandCenterApi";
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, ScheduledTask } from "../types/models";
import { pretty } from "../utils/format";
import { cloneJson, parseRequiredObject } from "../utils/json";
import { validateTask } from "../utils/tasks";
import { useAppStore } from "./app";

export const useTasksStore = defineStore("tasks", () => {
  const tasks = ref<ScheduledTask[]>([]);
  const selectedTaskId = ref<number | null>(null);
  const selectedRunId = ref(0);
  const runs = ref<RunSummary[]>([]);
  const chunks = ref<RunChunk[]>([]);
  const artifacts = ref<RunArtifact[]>([]);
  const taskEditorOpen = ref(false);
  const taskEditorCreating = ref(false);
  const taskEditorError = ref("");
  const taskDraft = reactive<ScheduledTask>(newTaskDraft());
  const taskJson = reactive({ configuration: "{}" });

  const app = useAppStore();

  const scheduledTasks = computed(() => tasks.value.filter((task) => task.configuration?.task_type !== "workflow"));
  const selectedTask = computed(() => tasks.value.find((task) => task.id === selectedTaskId.value) ?? null);
  const selectedRun = computed(() => runs.value.find((run) => run.id === selectedRunId.value) ?? null);
  const canRunTask = computed(() => Boolean(selectedTask.value?.id && selectedTask.value.enabled));
  const runOutput = computed(() => chunks.value.map((chunk) => `[${chunk.stream}] ${chunk.content}`).join("\n"));
  const recentRuns = computed(() => {
    const query = app.normalizedSearch;
    const list = query
      ? runs.value.filter((run) =>
          [run.id, run.status, run.trigger ?? "", run.message ?? ""].some((value) => String(value).toLowerCase().includes(query))
        )
      : runs.value;
    return list.slice(0, 50);
  });

  async function refreshTasks() {
    tasks.value = await app.runOperation("Loading tasks", () => fetchTasks()).catch(() => []);
    if (!selectedTaskId.value || !tasks.value.some((task) => task.id === selectedTaskId.value)) {
      selectedTaskId.value = scheduledTasks.value[0]?.id ?? tasks.value[0]?.id ?? null;
    }
    if (selectedTaskId.value) await refreshRunsForSelectedTask();
  }

  async function refreshRunsForSelectedTask() {
    if (!selectedTaskId.value) {
      runs.value = [];
      selectedRunId.value = 0;
      chunks.value = [];
      artifacts.value = [];
      return;
    }
    runs.value = await app.runOperation("Loading task runs", () => fetchTaskRuns(selectedTaskId.value!)).catch(() => []);
    if (!runs.value.some((run) => run.id === selectedRunId.value)) {
      selectedRunId.value = runs.value[0]?.id ?? 0;
    }
    if (selectedRunId.value) await selectRunById(selectedRunId.value);
  }

  async function selectRun(run: RunSummary) {
    await selectRunById(run.id);
  }

  async function selectRunById(runId: number) {
    selectedRunId.value = runId;
    const [nextChunks, nextArtifacts] = await Promise.all([
      app.runOperation("Loading run chunks", () => fetchRunChunks(runId)).catch(() => [] as RunChunk[]),
      app.runOperation("Loading run artifacts", () => fetchRunArtifacts(runId)).catch(() => [] as RunArtifact[])
    ]);
    chunks.value = nextChunks;
    artifacts.value = nextArtifacts;
  }

  async function runSelectedTask() {
    const task = selectedTask.value;
    if (!task?.id || !task.enabled) return;
    const response = await app.runOperation(`Running task ${task.name}`, () => requestTaskRun(task.id!));
    app.setStatus(response?.message || `Task queued: ${task.name}`);
    await refreshRunsForSelectedTask();
  }

  function openNewTask() {
    Object.assign(taskDraft, newTaskDraft());
    taskJson.configuration = "{}";
    taskEditorCreating.value = true;
    taskEditorError.value = "";
    taskEditorOpen.value = true;
  }

  function openSelectedTask() {
    if (!selectedTask.value) return;
    Object.assign(taskDraft, cloneJson(selectedTask.value));
    taskJson.configuration = pretty(taskDraft.configuration ?? {});
    taskEditorCreating.value = false;
    taskEditorError.value = "";
    taskEditorOpen.value = true;
  }

  function closeTaskEditor() {
    taskEditorOpen.value = false;
    taskEditorError.value = "";
  }

  async function submitTaskEditor() {
    const configuration = parseRequiredObject(taskJson.configuration);
    if (!configuration) {
      taskEditorError.value = "Configuration must be a JSON object";
      app.setError(taskEditorError.value);
      return;
    }
    taskDraft.configuration = configuration;
    const error = validateTask(taskDraft, taskJson);
    if (error) {
      taskEditorError.value = error;
      app.setError(error);
      return;
    }
    const saved = await app.runOperation("Saving task", () => saveTask(cloneJson(taskDraft), taskEditorCreating.value));
    if (saved.success === false) {
      taskEditorError.value = saved.message || "Failed to save task";
      app.setError(taskEditorError.value);
      return;
    }
    app.setStatus(saved.message || `Task saved: ${taskDraft.name}`);
    taskEditorOpen.value = false;
    await refreshTasks();
    if (saved.task?.id) selectedTaskId.value = saved.task.id;
  }

  function moveTaskSelection(delta: number) {
    const list = scheduledTasks.value.length ? scheduledTasks.value : tasks.value;
    if (list.length === 0) return;
    const current = list.findIndex((task) => task.id === selectedTaskId.value);
    selectedTaskId.value = list[boundedIndex(current, delta, list.length)].id;
    refreshRunsForSelectedTask();
  }

  function moveRunSelection(delta: number) {
    if (recentRuns.value.length === 0) return;
    const current = recentRuns.value.findIndex((run) => run.id === selectedRunId.value);
    selectRunById(recentRuns.value[boundedIndex(current, delta, recentRuns.value.length)].id);
  }

  return {
    tasks,
    scheduledTasks,
    selectedTaskId,
    selectedTask,
    selectedRunId,
    selectedRun,
    runs,
    recentRuns,
    chunks,
    artifacts,
    runOutput,
    canRunTask,
    taskEditorOpen,
    taskEditorCreating,
    taskEditorError,
    taskDraft,
    taskJson,
    refreshTasks,
    refreshRunsForSelectedTask,
    selectRun,
    runSelectedTask,
    openNewTask,
    openSelectedTask,
    closeTaskEditor,
    submitTaskEditor,
    moveTaskSelection,
    moveRunSelection
  };
});

export function newTaskDraft(): ScheduledTask {
  return {
    id: null,
    name: "",
    cron_schedule: "",
    action_name: "",
    action_function: "",
    enabled: true,
    timeout: 300,
    configuration: {}
  };
}

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}
