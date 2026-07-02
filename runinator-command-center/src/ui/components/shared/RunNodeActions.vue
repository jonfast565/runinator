<template>
  <div class="run-node-actions">
    <button
      type="button"
      class="btn btn-sm"
      :disabled="busy"
      title="Re-run the whole workflow"
      @click="emitAction('replay-run')"
    >
      <Icon name="replay" :size="13" />
      <span>Rerun</span>
    </button>
    <button
      v-if="canReplayFrom"
      type="button"
      class="btn btn-sm"
      :disabled="busy"
      title="Replay this run starting from this node"
      @click="emitAction('replay-from')"
    >
      <Icon name="restart" :size="13" />
      <span>Replay from here</span>
    </button>
    <button type="button" class="btn btn-sm" :title="copyTitle('input')" @click="copy('input')">
      <Icon name="download" :size="13" />
      <span>{{ copied === "input" ? "Copied" : "Copy input" }}</span>
    </button>
    <button type="button" class="btn btn-sm" :title="copyTitle('output')" @click="copy('output')">
      <Icon name="download" :size="13" />
      <span>{{ copied === "output" ? "Copied" : isFailed ? "Copy error" : "Copy output" }}</span>
    </button>
    <button
      v-if="showEditorActions"
      type="button"
      class="btn btn-sm"
      :disabled="busy"
      title="Open this step in the editor"
      @click="emitAction('open-editor')"
    >
      <Icon name="edit" :size="13" />
      <span>Step editor</span>
    </button>
    <button
      v-if="showEditorActions"
      type="button"
      class="btn btn-sm"
      :disabled="busy"
      title="Open the provider metadata for this step"
      @click="emitAction('open-provider')"
    >
      <Icon name="info" :size="13" />
      <span>Provider</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import Icon from "./Icon.vue";
import type { RunSummary, WorkflowNodeRun } from "../../../core/domain/models";

export type RunNodeActionType = "replay-run" | "replay-from" | "open-editor" | "open-provider";

const props = defineProps<{
  node: WorkflowNodeRun;
  run: RunSummary;
  busy?: boolean;
  showEditorActions?: boolean;
}>();

const emit = defineEmits<{
  action: [payload: { type: RunNodeActionType; node: WorkflowNodeRun }];
}>();

const copied = ref<"input" | "output" | "">("");
const FAILED_STATUSES = new Set(["failed", "timed_out"]);
const isFailed = computed(() => FAILED_STATUSES.has(props.node.status));

// start, end and fail are graph terminals; replaying "from" them is not meaningful.
const canReplayFrom = computed(() => {
  const id = props.node.node_id;
  return Boolean(id) && id !== "start" && id !== "end" && id !== "fail";
});

function emitAction(type: RunNodeActionType) {
  emit("action", { type, node: props.node });
}

function copyTitle(kind: "input" | "output"): string {
  return kind === "input"
    ? "Copy this node's input parameters"
    : "Copy this node's output (or error message)";
}

async function copy(kind: "input" | "output") {
  const text = kind === "input" ? toText(props.node.parameters) : errorOrOutput();

  try {
    await navigator.clipboard.writeText(text);
    copied.value = kind;
    window.setTimeout(() => {
      if (copied.value === kind) {
        copied.value = "";
      }
    }, 1200);
  } catch {
    // clipboard may be unavailable; silently ignore so the debug loop is not interrupted.
  }
}

// failed nodes copy the failure message first, falling back to the structured output.
function errorOrOutput(): string {
  if (isFailed.value && props.node.message) {
    return props.node.message;
  }

  const output = props.node.output_json;

  if (output === undefined || output === null) {
    return props.node.message ?? "";
  }

  return toText(output);
}

function toText(value: unknown): string {
  if (value === undefined || value === null) {
    return "";
  }

  if (typeof value === "string") {
    return value;
  }

  return JSON.stringify(value, null, 2);
}
</script>

<style scoped>
.run-node-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.btn-sm {
  font-size: 11px;
  padding: 3px 8px;
}
</style>
