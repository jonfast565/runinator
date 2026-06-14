<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="strokeWidth"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="icon"
    :class="`icon-${name}`"
    aria-hidden="true"
    focusable="false"
  >
    <template v-if="name === 'play'">
      <polygon points="6,4 20,12 6,20" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'pause'">
      <rect x="6" y="4" width="4" height="16" fill="currentColor" stroke="none" />
      <rect x="14" y="4" width="4" height="16" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'stop'">
      <rect x="5" y="5" width="14" height="14" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'restart'">
      <path d="M4 12a8 8 0 1 0 3-6.2" />
      <polyline points="3,3 3,8 8,8" />
    </template>
    <template v-else-if="name === 'replay'">
      <path d="M3 4v6h6" />
      <path d="M3.5 14a8.5 8.5 0 1 0 1.7-6.5l-2.2 2.5" />
    </template>
    <template v-else-if="name === 'step'">
      <polygon points="5,4 14,12 5,20" fill="currentColor" stroke="none" />
      <line x1="18" y1="4" x2="18" y2="20" />
    </template>
    <template v-else-if="name === 'continue'">
      <polygon points="4,4 14,12 4,20" fill="currentColor" stroke="none" />
      <polygon points="13,4 22,12 13,20" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'close' || name === 'x'">
      <line x1="6" y1="6" x2="18" y2="18" />
      <line x1="18" y1="6" x2="6" y2="18" />
    </template>
    <template v-else-if="name === 'plus'">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </template>
    <template v-else-if="name === 'minus'">
      <line x1="5" y1="12" x2="19" y2="12" />
    </template>
    <template v-else-if="name === 'trash'">
      <polyline points="3,6 5,6 21,6" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
      <path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2" />
    </template>
    <template v-else-if="name === 'edit'">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z" />
    </template>
    <template v-else-if="name === 'save'">
      <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
      <polyline points="17,21 17,13 7,13 7,21" />
      <polyline points="7,3 7,8 15,8" />
    </template>
    <template v-else-if="name === 'download'">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7,10 12,15 17,10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </template>
    <template v-else-if="name === 'upload'">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="17,8 12,3 7,8" />
      <line x1="12" y1="3" x2="12" y2="15" />
    </template>
    <template v-else-if="name === 'check'">
      <polyline points="20,6 9,17 4,12" />
    </template>
    <template v-else-if="name === 'alert'">
      <circle cx="12" cy="12" r="9" />
      <line x1="12" y1="7" x2="12" y2="13" />
      <line x1="12" y1="16" x2="12" y2="17" />
    </template>
    <template v-else-if="name === 'info'">
      <circle cx="12" cy="12" r="9" />
      <line x1="12" y1="11" x2="12" y2="16" />
      <line x1="12" y1="7" x2="12" y2="8" />
    </template>
    <template v-else-if="name === 'search'">
      <circle cx="11" cy="11" r="7" />
      <line x1="16.5" y1="16.5" x2="21" y2="21" />
    </template>
    <template v-else-if="name === 'file'">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14,2 14,8 20,8" />
    </template>
    <template v-else-if="name === 'folder'">
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </template>
    <template v-else-if="name === 'bell'">
      <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
      <path d="M10 21a2 2 0 0 0 4 0" />
    </template>
    <template v-else-if="name === 'settings'">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </template>
    <template v-else-if="name === 'refresh'">
      <polyline points="21,3 21,9 15,9" />
      <polyline points="3,21 3,15 9,15" />
      <path d="M3.51 9a9 9 0 0 1 14.85-3.36L21 9" />
      <path d="M20.49 15A9 9 0 0 1 5.64 18.36L3 15" />
    </template>
    <template v-else-if="name === 'debug'">
      <rect x="8" y="6" width="8" height="14" rx="4" />
      <line x1="12" y1="6" x2="12" y2="4" />
      <line x1="8" y1="10" x2="4" y2="8" />
      <line x1="16" y1="10" x2="20" y2="8" />
      <line x1="8" y1="14" x2="4" y2="14" />
      <line x1="16" y1="14" x2="20" y2="14" />
      <line x1="8" y1="18" x2="4" y2="20" />
      <line x1="16" y1="18" x2="20" y2="20" />
    </template>
    <template v-else-if="name === 'mail'">
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <polyline points="3,7 12,13 21,7" />
    </template>
    <template v-else-if="name === 'approve'">
      <circle cx="12" cy="12" r="9" />
      <polyline points="8,12 11,15 16,9" />
    </template>
    <template v-else-if="name === 'reject'">
      <circle cx="12" cy="12" r="9" />
      <line x1="9" y1="9" x2="15" y2="15" />
      <line x1="15" y1="9" x2="9" y2="15" />
    </template>
    <template v-else-if="name === 'arrow-up'">
      <line x1="12" y1="19" x2="12" y2="5" />
      <polyline points="5,12 12,5 19,12" />
    </template>
    <template v-else-if="name === 'arrow-down'">
      <line x1="12" y1="5" x2="12" y2="19" />
      <polyline points="19,12 12,19 5,12" />
    </template>
    <template v-else-if="name === 'chevron-left'">
      <polyline points="15,6 9,12 15,18" />
    </template>
    <template v-else-if="name === 'chevron-right'">
      <polyline points="9,6 15,12 9,18" />
    </template>
    <template v-else-if="name === 'workflow'">
      <circle cx="6" cy="6" r="3" />
      <circle cx="18" cy="6" r="3" />
      <circle cx="12" cy="18" r="3" />
      <line x1="8.5" y1="7.5" x2="11" y2="15" />
      <line x1="15.5" y1="7.5" x2="13" y2="15" />
    </template>
    <template v-else-if="name === 'runs'">
      <polyline points="4,7 8,11 4,15" />
      <polyline points="12,7 16,11 12,15" />
      <line x1="20" y1="5" x2="20" y2="19" />
    </template>
    <template v-else-if="name === 'list'">
      <line x1="8" y1="6" x2="20" y2="6" />
      <line x1="8" y1="12" x2="20" y2="12" />
      <line x1="8" y1="18" x2="20" y2="18" />
      <line x1="4" y1="6" x2="4.01" y2="6" />
      <line x1="4" y1="12" x2="4.01" y2="12" />
      <line x1="4" y1="18" x2="4.01" y2="18" />
    </template>
    <template v-else-if="name === 'key'">
      <circle cx="8" cy="14" r="4" />
      <line x1="11" y1="11" x2="21" y2="3" />
      <line x1="17" y1="7" x2="20" y2="10" />
      <line x1="14" y1="10" x2="17" y2="13" />
    </template>
    <template v-else-if="name === 'lock'">
      <rect x="5" y="10" width="14" height="11" rx="2" />
      <path d="M8 10V7a4 4 0 0 1 8 0v3" />
    </template>
    <template v-else-if="name === 'box'">
      <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
      <polyline points="3.27,6.96 12,12.01 20.73,6.96" />
      <line x1="12" y1="22.08" x2="12" y2="12" />
    </template>
    <template v-else-if="name === 'message'">
      <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z" />
    </template>
    <template v-else-if="name === 'gate'">
      <rect x="3" y="9" width="18" height="12" rx="1" />
      <path d="M7 9V6a5 5 0 0 1 10 0v3" />
    </template>
    <template v-else-if="name === 'gear'">
      <circle cx="12" cy="12" r="3" />
      <path d="M12 1v6m0 10v6m11-11h-6M7 12H1m18.07-7.07l-4.24 4.24M9.17 14.83l-4.24 4.24m0-14.14l4.24 4.24m5.66 5.66l4.24 4.24" />
    </template>
    <template v-else-if="name === 'flag'">
      <path d="M4 22V4a1 1 0 0 1 1.4-.9l13 5.4a1 1 0 0 1 0 1.8l-13 5.4" />
    </template>
    <template v-else-if="name === 'tag'">
      <path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" />
      <line x1="7" y1="7" x2="7.01" y2="7" />
    </template>
    <template v-else-if="name === 'cursor'">
      <polygon points="13,2 3,14 12,14 11,22 21,10 12,10" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'skip'">
      <polygon points="5,4 14,12 5,20" fill="currentColor" stroke="none" />
      <polygon points="14,4 22,12 14,20" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'circle'">
      <circle cx="12" cy="12" r="6" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'dot'">
      <circle cx="12" cy="12" r="4" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'breakpoint'">
      <circle cx="12" cy="12" r="6" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'bolt'">
      <polygon points="13,2 4,13 11,13 10,22 20,9 12,9" fill="currentColor" stroke="none" />
    </template>
    <template v-else-if="name === 'clock'">
      <circle cx="12" cy="12" r="9" />
      <polyline points="12,7 12,12 16,14" />
    </template>
    <template v-else-if="name === 'hourglass'">
      <path d="M7 3h10" />
      <path d="M7 21h10" />
      <path d="M8 4c0 4 4 4.5 4 8s-4 4-4 8" />
      <path d="M16 4c0 4-4 4.5-4 8s4 4 4 8" />
    </template>
    <template v-else-if="name === 'branch'">
      <path d="M12 3l9 9-9 9-9-9z" />
    </template>
    <template v-else-if="name === 'switch'">
      <path d="M4 7h6l4-3h6" />
      <path d="M4 12h16" />
      <path d="M4 17h6l4 3h6" />
    </template>
    <template v-else-if="name === 'loop'">
      <path d="M21 12a9 9 0 1 1-3-6.7" />
      <polyline points="21,3 21,8 16,8" />
    </template>
    <template v-else-if="name === 'parallel'">
      <line x1="4" y1="12" x2="12" y2="12" />
      <path d="M12 12l4-6h4" />
      <path d="M12 12l4 6h4" />
    </template>
    <template v-else-if="name === 'join'">
      <path d="M4 6h4l4 6" />
      <path d="M4 18h4l4-6" />
      <line x1="12" y1="12" x2="20" y2="12" />
    </template>
    <template v-else-if="name === 'shield'">
      <path d="M12 3l8 3v6c0 5-3.5 7.5-8 9-4.5-1.5-8-4-8-9V6z" />
      <polyline points="9,12 11,14 15,10" />
    </template>
    <template v-else-if="name === 'grid'">
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </template>
    <template v-else-if="name === 'race'">
      <circle cx="12" cy="13" r="7" />
      <line x1="12" y1="13" x2="12" y2="9" />
      <line x1="12" y1="13" x2="15" y2="13" />
      <line x1="9" y1="3" x2="15" y2="3" />
      <line x1="12" y1="3" x2="12" y2="6" />
    </template>
    <template v-else-if="name === 'output' || name === 'emit'">
      <circle cx="12" cy="12" r="2" fill="currentColor" stroke="none" />
      <path d="M8 8a5.6 5.6 0 0 0 0 8" />
      <path d="M16 8a5.6 5.6 0 0 1 0 8" />
      <path d="M5.5 5.5a9 9 0 0 0 0 13" />
      <path d="M18.5 5.5a9 9 0 0 1 0 13" />
    </template>
    <template v-else>
      <rect x="4" y="4" width="16" height="16" />
    </template>
  </svg>
</template>

<script setup lang="ts">
import { computed } from "vue";

export type IconName =
  | "play" | "pause" | "stop" | "restart" | "replay" | "step" | "continue"
  | "close" | "x" | "plus" | "minus" | "trash" | "edit" | "save"
  | "download" | "upload" | "check"
  | "alert" | "info" | "search"
  | "file" | "folder" | "bell" | "settings" | "refresh" | "debug" | "mail"
  | "approve" | "reject" | "arrow-up" | "arrow-down" | "chevron-left" | "chevron-right"
  | "workflow" | "runs" | "list" | "key" | "box" | "message" | "gate"
  | "gear" | "flag" | "tag" | "cursor" | "skip" | "circle" | "dot" | "breakpoint"
  | "bolt" | "clock" | "hourglass" | "branch" | "switch" | "loop" | "parallel" | "join"
  | "shield" | "grid" | "race" | "emit" | "output";

const props = withDefaults(
  defineProps<{ name: IconName | string; size?: number | string; strokeWidth?: number | string }>(),
  { size: 16, strokeWidth: 1.8 }
);

const size = computed(() => props.size);
const strokeWidth = computed(() => props.strokeWidth);
const name = computed(() => props.name);
void name;
</script>

<style scoped>
.icon {
  flex: 0 0 auto;
  display: inline-block;
  vertical-align: middle;
}
</style>
