<template>
  <div class="flex items-start gap-2 border-t border-fg-inverse-faint px-3 py-2">
    <span class="select-none pt-1 text-accent-pulse" aria-hidden="true">{{ sigil }}</span>
    <textarea
      ref="input"
      v-model="buffer"
      class="max-h-48 min-h-[1.5rem] flex-1 resize-none border-0 bg-transparent p-0 font-mono text-[12px] leading-6 text-fg-inverse outline-none placeholder:text-fg-inverse-faint focus:ring-0"
      :rows="rows"
      :placeholder="placeholder"
      :aria-label="placeholder"
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      @keydown="onKeydown"
    ></textarea>
    <button
      v-if="busy"
      class="btn btn-sm btn-danger"
      title="Stop the running line (Ctrl+C)"
      @click="emit('stop')"
    >
      Stop
    </button>
  </div>
  <!-- the completion menu and the key legend, laid out like the runinatorctl prompt's two bottom
       bands: candidates when there are any, what belongs at the caret when there are none, and the
       keys otherwise. -->
  <div
    v-if="menu.length"
    class="flex flex-wrap gap-x-4 gap-y-0.5 px-3 pb-1 font-mono text-[11px] text-accent-pulse"
  >
    <span v-for="option in menu" :key="option">{{ option }}</span>
  </div>
  <p v-else-if="hint" class="m-0 px-3 pb-1 font-mono text-[11px] text-fg-inverse-faint">
    {{ hint }}
  </p>
  <p class="m-0 px-3 pb-2 font-mono text-[11px] text-fg-inverse-faint">{{ LEGEND }}</p>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { complete, isSubmittable } from "../../../core/console/prompt";

const props = defineProps<{ busy: boolean; history: string[] }>();
const emit = defineEmits<{ submit: [line: string]; stop: []; clear: [] }>();

// the keys the prompt answers, worded as `runinatorctl`'s legend is so the two consoles read the
// same. the browser keeps Ctrl+D for itself, so the terminal's exit key has no counterpart here.
const LEGEND =
  "Enter run · Shift+Enter newline · Tab complete · ↑↓ history · Ctrl+C stop · Ctrl+L clear";

const buffer = ref("");
const input = ref<HTMLTextAreaElement | null>(null);
const menu = ref<string[]>([]);
// what belongs at the caret when Tab had nothing to insert, e.g. `<workflow>`.
const hint = ref<string | undefined>(undefined);
// where arrow-up currently is in the recalled history; -1 means "editing a fresh line".
const historyIndex = ref(-1);
const draft = ref("");

const rows = computed(() => Math.min(12, buffer.value.split("\n").length));
// the sigil says which of the two languages the line is in before it is submitted.
const sigil = computed(() => (buffer.value.trimStart().startsWith(":") ? ":" : "›"));
const placeholder = computed(() =>
  props.busy ? "running… (Ctrl+C stops)" : "REXRAP, or :help for commands",
);

// typing invalidates the menu, the way it does in a shell. the watcher is synchronous so that
// completing a word — which writes the buffer and then sets the menu — keeps its own candidates.
watch(
  buffer,
  () => {
    menu.value = [];
    hint.value = undefined;
  },
  { flush: "sync" },
);

watch(
  () => props.busy,
  (busy) => {
    if (!busy) {
      void focus();
    }
  },
);

async function focus() {
  await nextTick();
  input.value?.focus();
}

defineExpose({ focus });

function onKeydown(event: KeyboardEvent) {
  if (event.key === "c" && event.ctrlKey) {
    event.preventDefault();
    emit("stop");
    return;
  }

  if (event.key === "l" && event.ctrlKey) {
    event.preventDefault();
    emit("clear");
    return;
  }

  if (event.key === "Tab") {
    event.preventDefault();
    applyCompletion();
    return;
  }

  if (event.key === "Enter") {
    onEnter(event);
    return;
  }

  if (event.key === "ArrowUp" || event.key === "ArrowDown") {
    onHistory(event);
  }
}

// Enter submits a finished line and opens a new one otherwise; shift always means "new line", which
// is the escape hatch when the balance check disagrees with the author.
function onEnter(event: KeyboardEvent) {
  if (event.shiftKey || !isSubmittable(buffer.value)) {
    return;
  }

  event.preventDefault();

  if (props.busy) {
    return;
  }

  emit("submit", buffer.value);
  buffer.value = "";
  historyIndex.value = -1;
  menu.value = [];
  hint.value = undefined;
}

// arrow keys walk the history only from the edges of the buffer, so a multi-line cell can still be
// navigated with them.
function onHistory(event: KeyboardEvent) {
  const element = input.value;

  if (!element) {
    return;
  }

  const up = event.key === "ArrowUp";
  const atEdge = up ? element.selectionStart === 0 : element.selectionStart === buffer.value.length;

  if (!atEdge || props.history.length === 0) {
    return;
  }

  event.preventDefault();

  if (historyIndex.value === -1) {
    if (!up) {
      return;
    }

    draft.value = buffer.value;
    historyIndex.value = props.history.length;
  }

  const next = historyIndex.value + (up ? -1 : 1);

  if (next >= props.history.length) {
    historyIndex.value = -1;
    buffer.value = draft.value;
    return;
  }

  historyIndex.value = Math.max(0, next);
  buffer.value = props.history[historyIndex.value] ?? "";
}

// tab completes the word under the caret when there is one answer, and lists the choices when there
// are several — the same bargain a shell makes.
function applyCompletion() {
  const { start, options, hint: offered } = complete(buffer.value);

  if (options.length === 0) {
    // nothing to insert, but the usage line may still know what belongs here — saying so beats a
    // silent Tab.
    menu.value = [];
    hint.value = offered;
    return;
  }

  hint.value = undefined;

  if (options.length === 1) {
    buffer.value = `${buffer.value.slice(0, start)}${options[0]} `;
    menu.value = [];
    return;
  }

  const shared = commonPrefix(options);
  const typed = buffer.value.slice(start);

  if (shared.length > typed.length) {
    buffer.value = `${buffer.value.slice(0, start)}${shared}`;
  }

  menu.value = options;
}

function commonPrefix(values: string[]): string {
  return values.reduce((shared, value) => {
    let index = 0;

    while (index < shared.length && shared[index] === value[index]) {
      index += 1;
    }

    return shared.slice(0, index);
  });
}
</script>
