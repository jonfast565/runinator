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
import type { RunArtifact, RunChunk, RunSummary, ScheduledTask } from "../types/models";
import { cloneJson, parseObject } from "../utils/json";
import { pretty } from "../utils/format";
import { validateTask } from "../utils/tasks";
import { useAppStore } from "./app";

export const useTasksStore = defineStore("tasks", () => {
  const tasks = ref<ScheduledTask[]>([]);
  const selectedTaskId = ref<number | null>(null);
  const runs = ref<RunSummary[]>([]);
  const selectedRunId = ref(0);
  const chunks = ref<RunChunk[]>([]);
  const artifacts = ref<RunArtifact[]>([]);
  const taskEditorOpen = ref(false);
  const editingTaskCreating = ref(false);
  const taskEditorError = ref("");
  const taskDraft = reactive<ScheduledTask>(newTaskDraft());
  const taskJson = reactive({
    default_parameters: "{}",
    metadata: "{}"
  });

  const app = useAppStore();
  const selectedTask = computed(() => tasks.value.find((task) => task.id === selectedTaskId.value) ?? null);
  const selectedRun = computed(() => runs.value.find((run) => run.id === selectedRunId.value) ?? null);
  const canRunTask = computed(() => Boolean(selectedTask.value?.enabled && selectedTask.value.id));
  const runOutput = computed(() => chunks.value.map((chunk) => `[${chunk.stream}] ${chunk.content}`).join("\n"));
  const filteredTasks = computed(() => {
    const query = app.normalizedSearch;
    if (!query) return tasks.value;
    return tasks.value.filter((task) =>
      [task.name, task.cron_schedule, task.action_name, task.action_function].some((value) => value.toLowerCase().includes(query))
    );
  });

  const recentRuns = computed(() => {
    const query = app.normalizedSearch;
    let list = runs.value;
    if (query) {
      list = list.filter(r => 
        String(r.id).includes(query) || 
        (r.status && r.status.toLowerCase().includes(query))
      );
    }
    return list.slice(0, 50);
  });

  async function refreshTasks() {
    const loaded = await app.runOperation("Refreshing tasks", () => fetchTasks()).catch(() => []);
    tasks.value = loaded;
    if (!selectedTaskId.value && loaded.length > 0) selectedTaskId.value = loaded[0].id;
    if (selectedTaskId.value && !taskEditorOpen.value) await refreshRunsForSelectedTask();
    app.setStatus("Refreshed.");
  }

  async function refreshRunsForSelectedTask() {
    const task = selectedTask.value;
    if (!task?.id) {
      runs.value = [];
      chunks.value = [];
      artifacts.value = [];
      selectedRunId.value = 0;
      return;
    }
    runs.value = await app.runOperation("Refreshing runs", () => fetchTaskRuns(task.id!)).catch(() => []);
    if (!runs.value.some((run) => run.id === selectedRunId.value)) selectedRunId.value = runs.value[0]?.id ?? 0;
    const run = runs.value.find((item) => item.id === selectedRunId.value);
    if (run) await selectRun(run);
  }

  async function selectRun(run: RunSummary) {
    selectedRunId.value = run.id;
    const [loadedChunks, loadedArtifacts] = await Promise.all([
      app.runOperation("Loading chunks", () => fetchRunChunks(run.id)).catch(() => []),
      app.runOperation("Loading artifacts", () => fetchRunArtifacts(run.id)).catch(() => [])
    ]);
    chunks.value = loadedChunks;
    artifacts.value = loadedArtifacts;
  }

  async function selectTask(task: ScheduledTask) {
    if (taskEditorOpen.value && selectedTaskId.value !== task.id) {
       // Optionally warn or just don't allow changing task if editor is open and it's a different task
       // But usually the UI should prevent this. 
       // For now, let's just allow it but maybe it will overwrite the editor.
       // The user said "if you make changes, don't overwrite with refreshes".
    }
    selectedTaskId.value = task.id;
    return refreshRunsForSelectedTask();
  }

  async function runSelectedTask() {
    const task = selectedTask.value;
    if (!task?.id || !task.enabled) return app.setError(task ? "Task is disabled" : "No task selected");
    const response = await app.runOperation(`Running ${task.name}`, () => requestTaskRun(task.id!));
    app.setStatus(`${response.success === false ? "ERR" : "OK"}: ${response.message || 'Run requested'}`);
    await refreshRunsForSelectedTask();
  }

  function openNewTask() {
    Object.assign(taskDraft, newTaskDraft());
    taskJson.default_parameters = pretty(taskDraft.default_parameters);
    taskJson.metadata = pretty(taskDraft.metadata);
    editingTaskCreating.value = true;
    taskEditorError.value = "";
    taskEditorOpen.value = true;
  }

  function openSelectedTask() {
    if (selectedTask.value) openTask(selectedTask.value);
  }

  function openTask(task: ScheduledTask) {
    Object.assign(taskDraft, cloneJson(task));
    taskJson.default_parameters = pretty(task.default_parameters ?? {});
    taskJson.metadata = pretty(task.metadata ?? {});
    editingTaskCreating.value = false;
    taskEditorError.value = "";
    taskEditorOpen.value = true;
  }

  function closeTaskEditor() {
    taskEditorOpen.value = false;
  }

  async function submitTask() {
    const error = validateTaskDraft();
    if (error) {
      taskEditorError.value = error;
      return;
    }
    const task = cloneJson(taskDraft);
    task.default_parameters = parseObject(taskJson.default_parameters, {});
    task.metadata = parseObject(taskJson.metadata, {});
    if (!task.next_execution) task.next_execution = new Date().toISOString();
    const response = await app.runOperation(editingTaskCreating.value ? "Creating task" : "Updating task", () =>
      saveTask(task, editingTaskCreating.value)
    );
    if (response.success === false) {
      taskEditorError.value = response.message;
      return;
    }
    closeTaskEditor();
    app.setStatus(`OK: ${response.message || 'Task saved'}`);
    await refreshTasks();
  }

  function validateTaskDraft(): string {
    return validateTask(taskDraft, taskJson);
  }

  function moveTaskSelection(delta: number) {
    const list = filteredTasks.value;
    if (list.length === 0) return;
    const current = list.findIndex((task) => task.id === selectedTaskId.value);
    selectTask(list[boundedIndex(current, delta, list.length)]);
  }

  function moveRunSelection(delta: number) {
    const list = runs.value;
    if (list.length === 0) return;
    const current = list.findIndex((run) => run.id === selectedRunId.value);
    selectRun(list[boundedIndex(current, delta, list.length)]);
  }

  return {
    recentRuns,
    tasks,
    selectedTaskId,
    runs,
    selectedRunId,
    chunks,
    artifacts,
    taskEditorOpen,
    editingTaskCreating,
    taskEditorError,
    taskDraft,
    taskJson,
    selectedTask,
    selectedRun,
    canRunTask,
    runOutput,
    filteredTasks,
    refreshTasks,
    refreshRunsForSelectedTask,
    selectRun,
    selectTask,
    runSelectedTask,
    openNewTask,
    openSelectedTask,
    openTask,
    closeTaskEditor,
    submitTask,
    validateTaskDraft,
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
    timeout: 1,
    next_execution: null,
    enabled: true,
    immediate: false,
    blackout_start: null,
    blackout_end: null,
    default_parameters: {},
    mcp_enabled: false,
    metadata: {},
    tags: []
  };
}

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}
