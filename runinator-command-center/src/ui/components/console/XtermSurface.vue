<template>
  <div ref="host" class="xterm-host min-h-0 flex-1" @click="focus" />
</template>

<script setup lang="ts">
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    content: string;
    readonly?: boolean;
    ariaLabel?: string;
  }>(),
  { readonly: false, ariaLabel: "Terminal" },
);
const emit = defineEmits<{
  data: [data: string];
  resize: [size: { cols: number; rows: number }];
}>();

const host = ref<HTMLElement | null>(null);
let terminal: Terminal | null = null;
let fit: FitAddon | null = null;
let observer: ResizeObserver | null = null;
let rendered = "";

function render(content: string) {
  const instance = terminal;

  if (!instance) {
    return;
  }

  if (content.startsWith(rendered)) {
    instance.write(content.slice(rendered.length));
  } else {
    instance.reset();
    instance.write(content);
  }

  rendered = content;
}

function fitAndReport() {
  if (!terminal || !fit) {
    return;
  }

  try {
    fit.fit();
    emit("resize", { cols: terminal.cols, rows: terminal.rows });
  } catch {
    // A pane can briefly have zero dimensions while SplitPane changes layout.
  }
}

async function focus() {
  await nextTick();
  terminal?.focus();
}

onMounted(() => {
  if (!host.value) {
    return;
  }

  terminal = new Terminal({
    allowProposedApi: false,
    convertEol: false,
    cursorBlink: !props.readonly,
    cursorStyle: "block",
    disableStdin: props.readonly,
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    lineHeight: 1.25,
    scrollback: 10_000,
    theme: {
      background: "#11151b",
      foreground: "#e6ebf2",
      cursor: "#2de0d1",
      cursorAccent: "#11151b",
      selectionBackground: "#315c69",
      black: "#11151b",
      red: "#ff9b91",
      green: "#74d99f",
      yellow: "#f1c86d",
      blue: "#9ac9ff",
      magenta: "#d7a6ff",
      cyan: "#2de0d1",
      white: "#e6ebf2",
    },
  });
  fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(host.value);
  terminal.textarea?.setAttribute("aria-label", props.ariaLabel);
  terminal.onData((data) => { emit("data", data); });
  observer = new ResizeObserver(fitAndReport);
  observer.observe(host.value);
  render(props.content);
  fitAndReport();

  if (!props.readonly) {
    void focus();
  }
});

watch(
  () => props.content,
  (content) => { render(content); },
);

watch(
  () => props.readonly,
  (readonly) => {
    if (terminal) {
      terminal.options.disableStdin = readonly;
      terminal.options.cursorBlink = !readonly;
    }
  },
);

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
  terminal?.dispose();
  terminal = null;
  fit = null;
});

defineExpose({ focus, fit: fitAndReport });
</script>

<style scoped>
.xterm-host {
  min-height: 180px;
  overflow: hidden;
  background: #11151b;
  padding: 8px;
}

.xterm-host :deep(.xterm) {
  height: 100%;
}
</style>
