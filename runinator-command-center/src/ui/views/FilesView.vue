<template>
  <section class="pane h-full overflow-hidden">
    <div class="panel h-full min-h-0">
      <PanelHeader
        title="Files"
        icon="folder"
        eyebrow="Immutable library"
        description="Upload a file or folder with relative paths. Each upload creates an immutable revision that runs can pin safely."
      >
        <button class="btn" :disabled="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
        <label class="btn btn-primary cursor-pointer" :class="{ 'opacity-60': uploading }">
          <Icon name="upload" />
          <span>Upload files</span>
          <input
            class="sr-only"
            type="file"
            multiple
            :disabled="uploading > 0"
            aria-label="Upload files"
            @change="uploadFiles"
          />
        </label>
        <label class="btn btn-primary cursor-pointer" :class="{ 'opacity-60': uploading }">
          <Icon name="folder" />
          <span>Upload folder</span>
          <input
            class="sr-only"
            type="file"
            multiple
            :disabled="uploading > 0"
            aria-label="Upload folder"
            webkitdirectory=""
            directory=""
            @change="uploadFiles"
          />
        </label>
      </PanelHeader>
      <p v-if="error" class="mb-2 text-sm text-danger">{{ error }}</p>
      <p v-if="uploading" class="mb-2 text-sm text-fg-muted">
        Uploading {{ uploading }} file{{ uploading === 1 ? "" : "s" }}…
      </p>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <MetricCard label="Visible files" :value="filteredFiles.length" />
        <MetricCard label="Stored size" :value="formatBytes(totalBytes)" />
        <MetricCard label="Latest revision" :value="latestRevisionLabel" />
      </div>
      <LoadingPanel v-if="loading && !files.length" compact message="Loading file library…" />
      <EmptyState
        v-else-if="!filteredFiles.length"
        compact
        :icon="files.length ? 'search' : 'folder'"
        :title="files.length ? 'No matches' : 'No reusable files yet'"
        :description="
          files.length
            ? `No files match “${app.searchQuery}”.`
            : 'Upload a file or folder to create an immutable revision workflows can pin.'
        "
      />
      <div v-else class="table-scroll min-h-0 flex-1">
        <DataTable bare table-class="entity-banner-table table-resize-disabled">
          <thead>
            <tr>
              <th>File</th>
              <th class="entity-banner-actions"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="file in filteredFiles" :key="file.descriptor.id">
              <td :title="file.descriptor.path">
                <div class="entity-banner-content">
                  <span class="entity-banner-title font-mono text-[12px]">
                    {{ file.descriptor.path }}
                  </span>
                  <span class="entity-banner-meta">
                    v{{ file.revision }} · {{ file.descriptor.mime_type }} ·
                    {{ formatBytes(file.descriptor.size_bytes) }} ·
                    {{ formatDate(file.created_at) }}
                  </span>
                </div>
              </td>
              <td class="entity-banner-actions whitespace-nowrap">
                <button class="btn btn-sm" type="button" @click="download(file)">
                  <Icon name="download" :size="13" /> Download
                </button>
                <button class="btn btn-sm btn-danger ml-1" type="button" @click="archive(file)">
                  <Icon name="trash" :size="13" />
                </button>
              </td>
            </tr>
          </tbody>
        </DataTable>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  archiveWorkflowFile,
  downloadWorkflowFileContent,
  fetchWorkflowFiles,
  uploadWorkflowFile,
} from "../../core/api/commandCenterApi";
import type { WorkflowFile } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";
import { downloadBlob } from "../adapters/browser/files";
import Icon from "../components/shared/Icon.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import { useAppStore } from "../adapters/pinia/app";

const app = useAppStore();
const files = ref<WorkflowFile[]>([]);
const loading = ref(false);
const uploading = ref(0);
const error = ref("");
const filteredFiles = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return files.value;
  }

  return files.value.filter((file) =>
    [file.descriptor.path, file.descriptor.name, file.descriptor.mime_type].some((value) =>
      value.toLowerCase().includes(query),
    ),
  );
});
const totalBytes = computed(() =>
  filteredFiles.value.reduce((total, file) => total + file.descriptor.size_bytes, 0),
);
const latestRevisionLabel = computed(() => {
  const revision = Math.max(0, ...filteredFiles.value.map((file) => file.revision));
  return revision ? `v${String(revision)}` : "—";
});

async function refresh() {
  loading.value = true;
  error.value = "";

  try {
    files.value = await fetchWorkflowFiles();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

function relativePath(file: File) {
  const path = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
  return path.replace(/\\/g, "/").replace(/^\/+/, "");
}

function uploadSelectionError(entries: File[]): string {
  const paths = entries.map(relativePath);
  const invalidPath = paths.find((path) => {
    const segments = path.split("/");
    return (
      !path ||
      path.length > 512 ||
      segments.some((segment) => !segment || segment === "." || segment === "..")
    );
  });

  if (invalidPath !== undefined) {
    return `“${invalidPath || "Unnamed file"}” does not have a valid relative path.`;
  }

  if (new Set(paths).size !== paths.length) {
    return "The selection contains duplicate relative file paths.";
  }

  return "";
}

async function uploadFiles(event: Event) {
  const input = event.target as HTMLInputElement;
  const entries = [...(input.files ?? [])];
  input.value = "";

  if (!entries.length) {
    return;
  }

  const selectionError = uploadSelectionError(entries);

  if (selectionError) {
    error.value = selectionError;
    return;
  }

  uploading.value = entries.length;
  error.value = "";

  try {
    for (const file of entries) {
      await uploadWorkflowFile(relativePath(file), file);
    }

    await refresh();
  } catch (reason) {
    error.value = `Upload failed: ${String(reason)}`;
  } finally {
    uploading.value = 0;
  }
}

async function download(file: WorkflowFile) {
  try {
    downloadBlob(file.descriptor.name, await downloadWorkflowFileContent(file.descriptor.id));
  } catch (reason) {
    error.value = `Download failed: ${String(reason)}`;
  }
}

async function archive(file: WorkflowFile) {
  if (
    !window.confirm(
      `Archive ${file.descriptor.path}? Existing runs will keep their pinned revision.`,
    )
  ) {
    return;
  }

  try {
    await archiveWorkflowFile(file.descriptor.id);
    await refresh();
  } catch (reason) {
    error.value = `Archive failed: ${String(reason)}`;
  }
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

onMounted(refresh);
</script>
