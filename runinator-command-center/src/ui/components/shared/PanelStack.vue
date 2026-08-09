<template>
  <div class="panel-stack" :class="{ 'panel-stack-collapsed': collapsed }">
    <nav class="panel-stack-tabs" role="tablist">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        type="button"
        role="tab"
        class="panel-stack-tab"
        :class="{ active: !collapsed && tab.id === activeId }"
        :aria-selected="!collapsed && tab.id === activeId"
        :title="tab.title ?? tab.label"
        @click="select(tab.id)"
      >
        <Icon v-if="tab.icon" :name="tab.icon" :size="13" />
        <span>{{ tab.label }}</span>
        <span v-if="tab.badge" class="count-pill" :class="tab.badgeTone ?? 'error'">{{
          tab.badge
        }}</span>
      </button>

      <button
        v-if="collapsible"
        type="button"
        class="panel-stack-collapse"
        :title="collapsed ? 'Restore panel' : 'Minimize panel'"
        :aria-label="collapsed ? 'Restore panel' : 'Minimize panel'"
        @click="toggleCollapsed"
      >
        <Icon :name="collapsed ? 'chevron-left' : 'chevron-right'" :size="13" />
      </button>
    </nav>

    <!-- every tab's body stays mounted so an editor does not lose its state on a tab switch, the
         way an Eclipse view stack keeps its views alive behind the active one. -->
    <div v-show="!collapsed" class="panel-stack-body">
      <div
        v-for="tab in tabs"
        v-show="tab.id === activeId"
        :key="tab.id"
        class="panel-stack-view"
        role="tabpanel"
      >
        <slot :name="tab.id" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * an eclipse-style view stack: several panels docked on the same side, one visible at a time, with
 * the whole stack collapsible to its tab strip.
 *
 * the active tab and the collapsed flag persist under `storageKey`, matching how `SplitPane` keeps
 * its divider position, so a layout survives a reload. `modelValue` is optional -- pass it when the
 * active tab is application state someone else also sets (the workflow inspector switches to its
 * step tab whenever a canvas node is clicked), and leave it off for a purely local stack.
 */
import { computed, onMounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import type { PanelStackTab } from "./panel-stack";

const props = withDefaults(
  defineProps<{
    tabs: PanelStackTab[];
    modelValue?: string;
    storageKey?: string;
    collapsible?: boolean;
  }>(),
  { modelValue: "", storageKey: "", collapsible: true },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const localId = ref(props.modelValue || props.tabs.at(0)?.id || "");
const collapsed = ref(false);

const activeId = computed(() => {
  const id = props.modelValue || localId.value;
  return props.tabs.some((tab) => tab.id === id) ? id : (props.tabs.at(0)?.id ?? "");
});

const collapsedKey = computed(() =>
  props.storageKey ? `${props.storageKey}::collapsed` : "",
);

function select(id: string) {
  // clicking the active tab of an expanded stack minimizes it, which is how eclipse's tabs behave.
  if (id === activeId.value && !collapsed.value && props.collapsible) {
    toggleCollapsed();
    return;
  }

  collapsed.value = false;
  localId.value = id;
  emit("update:modelValue", id);
  persist();
}

function toggleCollapsed() {
  collapsed.value = !collapsed.value;
  persist();
}

function persist() {
  if (!props.storageKey) {
    return;
  }

  // only an uncontrolled stack persists its active tab. when the parent owns the value it also owns
  // what the tab should be on load, so writing an id nothing reads back would be dead state.
  if (!props.modelValue) {
    window.localStorage.setItem(props.storageKey, activeId.value);
  }

  window.localStorage.setItem(collapsedKey.value, collapsed.value ? "1" : "");
}

watch(
  () => props.modelValue,
  (next) => {
    if (!next) {
      return;
    }

    localId.value = next;
    // a programmatic switch means something wants to be seen, so restore a minimized stack.
    collapsed.value = false;
  },
);

onMounted(() => {
  if (!props.storageKey) {
    return;
  }

  const savedId = window.localStorage.getItem(props.storageKey);

  if (savedId && props.tabs.some((tab) => tab.id === savedId) && !props.modelValue) {
    localId.value = savedId;
    emit("update:modelValue", savedId);
  }

  collapsed.value = props.collapsible && window.localStorage.getItem(collapsedKey.value) === "1";
});
</script>
