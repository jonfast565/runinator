<template>
  <div ref="tree" class="folder-tree" role="tree" aria-label="Workspace folders">
    <button
      v-for="(folder, index) in folders"
      :key="folder.path"
      type="button"
      role="treeitem"
      :aria-level="folder.depth + 1"
      :aria-expanded="expanded.has(folder.path)"
      :aria-selected="selected === folder.path"
      :tabindex="focused === folder.path ? 0 : -1"
      :style="{ paddingLeft: `${String(8 + folder.depth * 16)}px` }"
      :title="folder.path || '/'"
      @focus="focused = folder.path"
      @click="select(folder.path)"
      @keydown="keydown($event, index)"
    >
      <span @click.stop="toggle(folder.path)"
        ><Icon :name="expanded.has(folder.path) ? 'arrow-down' : 'chevron-right'" :size="14"
      /></span>
      <Icon name="folder" :size="15" />
      <span class="folder-name">{{ folder.name }}</span>
      <span v-if="directories[folder.path]?.loading" aria-label="Loading">…</span>
      <span
        v-if="directories[folder.path]?.error"
        title="Could not load folder; collapse and expand to retry"
        >!</span
      >
    </button>
  </div>
</template>
<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import type { DirectoryState } from "../../../core/services/workspace-directories";
import Icon from "../shared/Icon.vue";
const props = defineProps<{
  directories: Partial<Record<string, DirectoryState>>;
  selected: string;
}>();
const emit = defineEmits<{
  select: [path: string];
  expand: [path: string];
  collapse: [path: string];
}>();
const expanded = ref(new Set([""]));
onMounted(() => {
  emit("expand", "");
});
const focused = ref("");
const tree = ref<HTMLElement>();
const folders = computed(() => {
  const result: { path: string; name: string; depth: number }[] = [];
  const stack = [{ path: "", name: "/", depth: 0 }];

  while (stack.length) {
    const folder = stack.pop();

    if (!folder) {
      break;
    }

    result.push(folder);

    if (!expanded.value.has(folder.path)) {
      continue;
    }

    const children =
      props.directories[folder.path]?.entries.filter((entry) => entry.kind === "directory") ?? [];

    for (const entry of children.slice().reverse()) {
      stack.push({
        path: folder.path ? `${folder.path}/${entry.name}` : entry.name,
        name: entry.name,
        depth: folder.depth + 1,
      });
    }
  }

  return result;
});
watch(
  () => props.selected,
  (path) => {
    focused.value = path;
    const parts = path.split("/");

    for (let i = 0; i < parts.length; i++) {
      const ancestor = parts.slice(0, i).join("/");
      expanded.value.add(ancestor);
      emit("expand", ancestor);
    }
  },
);

function select(path: string) {
  focused.value = path;
  emit("select", path);
}

function toggle(path: string) {
  if (expanded.value.has(path)) {
    expanded.value.delete(path);
    emit("collapse", path);
    return;
  }

  expanded.value.add(path);
  emit("expand", path);
}

async function keydown(event: KeyboardEvent, index: number) {
  const folder = folders.value[index];

  if (!folder) {
    return;
  }

  let target = index;

  switch (event.key) {
    case "ArrowDown":
      target = Math.min(index + 1, folders.value.length - 1);
      break;
    case "ArrowUp":
      target = Math.max(index - 1, 0);
      break;
    case "Home":
      target = 0;
      break;
    case "End":
      target = folders.value.length - 1;
      break;
    case "ArrowRight":
      if (!expanded.value.has(folder.path)) {
        toggle(folder.path);
      } else if (folders.value[index + 1]?.depth === folder.depth + 1) {
        target++;
      }

      break;
    case "ArrowLeft":
      if (expanded.value.has(folder.path)) {
        toggle(folder.path);
      } else {
        target = Math.max(
          0,
          folders.value.findIndex(
            (item) => item.path === folder.path.split("/").slice(0, -1).join("/"),
          ),
        );
      }

      break;
    default:
      return;
  }

  event.preventDefault();
  focused.value = folders.value[target]?.path ?? "";
  await nextTick();
  tree.value?.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')[target]?.focus();
}
</script>
<style scoped>
.folder-tree {
  width: 100%;
  overflow: auto;
  height: 460px;
}
.folder-tree button {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
  min-height: 32px;
  cursor: pointer;
}
.folder-tree button[aria-selected="true"] {
  background: var(--surface-selected, var(--surface-subtle));
}
.folder-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
