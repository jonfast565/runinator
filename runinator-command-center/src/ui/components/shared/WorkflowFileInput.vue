<template>
  <div class="grid gap-2 rounded border border-border-subtle bg-surface-raised p-2">
    <div class="flex flex-wrap gap-2">
      <label class="btn btn-sm cursor-pointer">
        <Icon name="upload" :size="13" />
        <span>{{ multiple ? "Upload files" : "Upload file" }}</span>
        <input class="sr-only" type="file" :multiple="multiple" @change="stageFiles" />
      </label>
      <label v-if="multiple" class="btn btn-sm cursor-pointer">
        <Icon name="upload" :size="13" />
        <span>Upload folder</span>
        <input
          class="sr-only"
          type="file"
          multiple
          webkitdirectory=""
          directory=""
          @change="stageFiles"
        />
      </label>
      <select class="min-w-40 flex-1" :disabled="loadingLibrary" @change="addLibraryFile">
        <option value="">{{ loadingLibrary ? "Loading library…" : "Choose from library…" }}</option>
        <option v-for="file in libraryFiles" :key="file.descriptor.id" :value="file.descriptor.id">
          {{ file.descriptor.path }} · v{{ file.revision }}
        </option>
      </select>
    </div>
    <div v-if="error" class="text-xs text-danger">{{ error }}</div>
    <div v-if="uploading" class="text-xs text-fg-muted">Uploading {{ uploading }} file{{ uploading === 1 ? "" : "s" }}…</div>
    <ul v-if="selected.length" class="m-0 grid list-none gap-1 p-0 text-xs">
      <li v-for="file in selected" :key="file.id" class="flex items-center justify-between gap-2 rounded bg-surface px-2 py-1">
        <span class="min-w-0 truncate" :title="file.path">{{ file.path }}</span>
        <span class="shrink-0 text-fg-muted">{{ formatBytes(file.size_bytes) }}</span>
        <button class="btn btn-sm shrink-0" type="button" @click="remove(file.id)">Remove</button>
      </li>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import type { FileDescriptor, WorkflowFile } from "../../../core/domain/models";
import { fetchWorkflowFiles, uploadWorkflowFile } from "../../../core/api/commandCenterApi";
import Icon from "./Icon.vue";

const props = defineProps<{ modelValue: FileDescriptor | FileDescriptor[] | null | undefined; multiple?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: FileDescriptor | FileDescriptor[] | null] }>();

const libraryFiles = ref<WorkflowFile[]>([]);
const loadingLibrary = ref(false);
const uploading = ref(0);
const error = ref("");
const selected = ref<FileDescriptor[]>([]);

function normalize(value = props.modelValue): FileDescriptor[] {
  if (Array.isArray(value)) {
    return value;
  }

  return value ? [value] : [];
}

watch(
  () => props.modelValue,
  (value) => { selected.value = normalize(value); },
  { immediate: true, deep: true },
);

onMounted(async () => {
  loadingLibrary.value = true;

  try {
    libraryFiles.value = await fetchWorkflowFiles();
  } catch (reason) {
    error.value = `Could not load file library: ${String(reason)}`;
  } finally {
    loadingLibrary.value = false;
  }
});

function emitSelected(next: FileDescriptor[]) {
  selected.value = next;
  emit("update:modelValue", props.multiple ? next : (next[0] ?? null));
}

function safeRelativePath(file: File): string {
  const relative = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
  return relative
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .split("/")
    .filter((part) => part && part !== "." && part !== "..")
    .join("/");
}

async function stageFiles(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = [...(input.files ?? [])];
  input.value = "";

  if (!files.length) {
    return;
  }

  error.value = "";
  uploading.value = files.length;

  try {
    const next = [...selected.value];

    for (const file of files) {
      const uploaded = await uploadWorkflowFile(safeRelativePath(file), file, true);

      if (!props.multiple) {
        next.splice(0, next.length, uploaded.descriptor);
        break;
      }

      next.push(uploaded.descriptor);
    }

    emitSelected(dedupe(next));
  } catch (reason) {
    error.value = `Upload failed: ${String(reason)}`;
  } finally {
    uploading.value = 0;
  }
}

function addLibraryFile(event: Event) {
  const select = event.target as HTMLSelectElement;
  const id = select.value;
  select.value = "";
  const descriptor = libraryFiles.value.find((file) => file.descriptor.id === id)?.descriptor;

  if (!descriptor) {
    return;
  }

  emitSelected(props.multiple ? dedupe([...selected.value, descriptor]) : [descriptor]);
}

function remove(id: string) {
  emitSelected(selected.value.filter((file) => file.id !== id));
}

function dedupe(files: FileDescriptor[]) {
  return [...new Map(files.map((file) => [file.id, file])).values()];
}

function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${String(bytes)} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
</script>
