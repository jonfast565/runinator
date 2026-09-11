<template>
  <div class="workspace-file-browser" :class="{ 'show-folders': showFolders }">
    <div class="browser-toolbar">
      <button class="btn btn-sm folder-toggle" @click="showFolders = !showFolders">Folders</button>
      <button class="btn btn-sm" :disabled="!path" @click="open(parent)">Parent</button>
      <nav aria-label="Directory breadcrumbs">
        <button class="btn btn-sm btn-ghost" @click="open('')">/</button>
        <button
          v-for="crumb in crumbs"
          :key="crumb.path"
          class="btn btn-sm btn-ghost"
          @click="open(crumb.path)"
        >
          {{ crumb.name }}
        </button>
      </nav>
      <input
        v-model="query"
        type="search"
        aria-label="Filter directory"
        placeholder="Filter this directory"
      />
    </div>
    <SplitPane
      :initial-first-pct="24"
      :min-first="160"
      :min-second="260"
      collapsible-first
      first-label="Folders"
      first-icon="folder"
      storage-key="workspace-file-browser"
    >
      <template #first>
        <WorkspaceFolderTree
          :directories="directories"
          :selected="path"
          @select="open"
          @expand="emit('expand', $event)"
          @collapse="emit('collapse', $event)"
        />
      </template>
      <template #second>
        <div class="directory-pane">
          <div class="directory-status" role="status">
            {{ filtered.length }}{{ query ? " matching" : "" }} entries{{
              current?.complete ? "" : " loaded"
            }}
            <span v-if="current?.loading"> · Loading…</span>
          </div>
          <div v-if="current?.error" role="alert" class="directory-error">
            {{ current.error }}
            <button class="btn btn-sm" @click="emit('open', path)">Retry</button>
          </div>
          <p v-if="current?.complete && !filtered.length">
            {{ query ? "No matching files or folders." : "This directory is empty." }}
          </p>
          <div
            ref="viewport"
            class="directory-scroll"
            @scroll="scrollTop = ($event.target as HTMLElement).scrollTop"
          >
            <table
              class="table-resize-disabled"
              aria-label="Directory contents"
              :aria-rowcount="filtered.length + 1"
            >
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Size</th>
                  <th class="digest">Digest</th>
                  <th><span class="sr-only">Download</span></th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="start" aria-hidden="true">
                  <td
                    colspan="4"
                    :style="{ height: `${String(start * rowHeight)}px`, padding: 0 }"
                  />
                </tr>
                <tr
                  v-for="(file, index) in windowEntries"
                  :key="file.name"
                  :aria-rowindex="start + index + 2"
                  class="entry-row"
                >
                  <td>
                    <div class="file-name">
                      <Icon
                        :name="
                          file.kind === 'directory'
                            ? 'folder'
                            : file.kind === 'symlink'
                              ? 'link'
                              : 'file'
                        "
                        :size="15"
                      />
                      <button
                        class="btn btn-sm btn-ghost"
                        :disabled="file.kind === 'symlink'"
                        :title="file.link_target ? `${file.name} → ${file.link_target}` : file.name"
                        @click="
                          file.kind === 'directory'
                            ? open(child(file.name))
                            : emit('preview', child(file.name))
                        "
                      >
                        {{ file.name }}
                      </button>
                      <small v-if="file.kind === 'file' && file.executable" title="Executable"
                        >*</small
                      >
                    </div>
                  </td>
                  <td>{{ file.kind === "directory" ? "—" : bytes(file.size_bytes) }}</td>
                  <td class="digest">
                    <code :title="file.content_id">{{ file.content_id.slice(0, 10) }}</code>
                  </td>
                  <td>
                    <button
                      v-if="file.kind === 'file'"
                      class="btn btn-sm btn-icon"
                      :disabled="busy"
                      :aria-label="`Download ${file.name}`"
                      @click="emit('download', child(file.name))"
                    >
                      <Icon name="download" :size="14" />
                    </button>
                  </td>
                </tr>
                <tr v-if="end < filtered.length" aria-hidden="true">
                  <td
                    colspan="4"
                    :style="{
                      height: `${String((filtered.length - end) * rowHeight)}px`,
                      padding: 0,
                    }"
                  />
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </template>
    </SplitPane>
  </div>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DirectoryState } from "../../../core/services/workspace-directories";
import SplitPane from "../shared/SplitPane.vue";
import Icon from "../shared/Icon.vue";
import WorkspaceFolderTree from "./WorkspaceFolderTree.vue";
const props = defineProps<{
  directories: Partial<Record<string, DirectoryState>>;
  path: string;
  busy: boolean;
}>();
const emit = defineEmits<{
  open: [path: string];
  expand: [path: string];
  collapse: [path: string];
  preview: [path: string];
  download: [path: string];
}>();
const query = ref("");
const showFolders = ref(false);
const viewport = ref<HTMLElement>();
const scrollTop = ref(0);
const height = ref(420);
const rowHeight = 40;
const current = computed(() => props.directories[props.path]);
const parent = computed(() => props.path.split("/").slice(0, -1).join("/"));
const crumbs = computed(() =>
  props.path
    ? props.path
        .split("/")
        .map((name, index, parts) => ({ name, path: parts.slice(0, index + 1).join("/") }))
    : [],
);
const filtered = computed(() => {
  const search = query.value.trim().toLowerCase();
  return (current.value?.entries ?? []).filter(
    (file) =>
      !search ||
      [file.name, file.content_id, file.link_target ?? ""].some((value) =>
        value.toLowerCase().includes(search),
      ),
  );
});
const start = computed(() =>
  Math.max(0, Math.min(filtered.value.length, Math.floor(scrollTop.value / rowHeight) - 5)),
);
const end = computed(() =>
  Math.min(filtered.value.length, start.value + Math.ceil(height.value / rowHeight) + 12),
);
const windowEntries = computed(() => filtered.value.slice(start.value, end.value));

function child(name: string) {
  return props.path ? `${props.path}/${name}` : name;
}

function open(path: string) {
  showFolders.value = false;
  emit("open", path);
}

function bytes(value: number) {
  if (value < 1024) {
    return `${String(value)} B`;
  }

  if (value < 1048576) {
    return `${(value / 1024).toFixed(1)} KiB`;
  }

  return `${(value / 1048576).toFixed(1)} MiB`;
}

watch(
  () => props.path,
  () => {
    query.value = "";
  },
);
watch([() => props.path, query], () => {
  scrollTop.value = 0;

  if (viewport.value) {
    viewport.value.scrollTop = 0;
  }
});
let observer: ResizeObserver | undefined;
onMounted(() => {
  observer = new ResizeObserver(() => {
    height.value = viewport.value?.clientHeight ?? 420;
  });

  if (viewport.value) {
    observer.observe(viewport.value);
  }
});
onBeforeUnmount(() => observer?.disconnect());
</script>
<style scoped>
.directory-pane {
  display: flex;
  min-height: 0;
  width: 100%;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
}
.workspace-file-browser {
  display: flex;
  min-height: 0;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
}
.workspace-file-browser > :deep(.split-pane) {
  min-height: 0;
  flex: 1 1 auto;
}
.browser-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.browser-toolbar nav {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  min-width: 0;
}
.browser-toolbar input {
  min-width: 120px;
  max-width: 100%;
}
.folder-toggle {
  display: none;
}
.directory-status {
  padding: 8px 0;
  color: var(--text-muted);
}
.directory-error {
  padding: 8px;
}
.directory-scroll {
  min-height: 0;
  flex: 1 1 auto;
  overflow: auto;
}
table {
  border-collapse: collapse;
  table-layout: fixed;
  width: 100%;
}
th {
  text-align: left;
  height: 32px;
}
th:first-child {
  width: 55%;
}
th:last-child {
  width: 40px;
}
td {
  padding: 0 4px;
}
.entry-row {
  height: 40px;
}
.entry-row td {
  border-bottom: 1px solid var(--border-subtle);
  white-space: nowrap;
  overflow: hidden;
}
.file-name {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}
.file-name button {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: block;
}
@media (max-width: 1024px) {
  .folder-toggle {
    display: inline-flex;
  }
  .workspace-file-browser:not(.show-folders) :deep(.split-section-first) {
    display: none;
  }
  .workspace-file-browser.show-folders :deep(.folder-tree) {
    height: 220px;
  }
}
@media (max-width: 600px) {
  .digest {
    display: none;
  }
}
</style>
