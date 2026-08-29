<template>
  <div class="workflow-terminal terminal-surface">
    <div class="workflow-terminal-toolbar">
      <div>
        <strong>Interactive terminal</strong>
        <span>{{ active ? "connected" : "read only" }}</span>
      </div>
      <span v-if="error" class="text-danger-fg">{{ error }}</span>
      <span v-else class="text-fg-inverse-faint">
        {{ active ? "Click the terminal and type; Ctrl+C is sent to the process." : "Session ended." }}
      </span>
    </div>
    <XtermSurface
      ref="surface"
      :content="content"
      :readonly="!active"
      aria-label="Interactive workflow terminal"
      @data="onData"
      @resize="onResize"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import {
  controlWorkflowEffectTerminal,
  type WorkflowTerminalControl,
} from "../../../core/api/commandCenterApi";
import type { RunChunk } from "../../../core/domain/models";
import XtermSurface from "../console/XtermSurface.vue";

const props = defineProps<{ effectId: string; chunks: RunChunk[]; active: boolean }>();
const surface = ref<InstanceType<typeof XtermSurface> | null>(null);
const error = ref("");
let bufferedInput = "";
let flushTimer: ReturnType<typeof setTimeout> | null = null;
let delivery = Promise.resolve();

const content = computed(() =>
  props.chunks
    .filter((chunk) => chunk.stream === "terminal")
    .map((chunk) => chunk.content)
    .join(""),
);

function queue(control: WorkflowTerminalControl) {
  if (!props.active) {
    return;
  }

  delivery = delivery
    .then(() => controlWorkflowEffectTerminal(props.effectId, control))
    .then(() => {
      error.value = "";
    })
    .catch((reason: unknown) => {
      error.value = reason instanceof Error ? reason.message : String(reason);
    });
}

function flushInput() {
  flushTimer = null;
  const data = bufferedInput;
  bufferedInput = "";

  if (data) {
    queue({ type: "input", data });
  }
}

function onData(data: string) {
  if (data === "\x04") {
    flushInput();
    queue({ type: "eof" });
    return;
  }

  bufferedInput += data;

  flushTimer ??= setTimeout(flushInput, 20);
}

function onResize(size: { cols: number; rows: number }) {
  queue({ type: "resize", cols: size.cols, rows: size.rows });
}

watch(
  () => props.effectId,
  () => {
    error.value = "";
    bufferedInput = "";
    void surface.value?.focus();
  },
);

onBeforeUnmount(() => {
  if (flushTimer) {
    clearTimeout(flushTimer);
  }

  flushInput();
});
</script>

<style scoped>
.workflow-terminal {
  display: flex;
  min-height: 320px;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}

.workflow-terminal-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 7px 10px;
  border-bottom: 1px solid rgba(230, 235, 242, 0.12);
  font-size: 11px;
}

.workflow-terminal-toolbar div {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
</style>
