<template>
  <section class="pane h-full overflow-hidden">
    <div class="panel h-full min-h-0">
      <PanelHeader
        title="Files"
        description="Upload a file or folder with relative paths. Each upload creates an immutable revision that runs can pin safely."
      >
        <button class="btn" :disabled="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
        <label class="btn btn-primary cursor-pointer" :class="{ 'opacity-60': uploading }">
          <Icon name="upload" />
          <span>Upload files</span>
          <input class="sr-only" type="file" multiple @change="uploadFiles" />
        </label>
        <label class="btn btn-primary cursor-pointer" :class="{ 'opacity-60': uploading }">
          <Icon name="folder" />
          <span>Upload folder</span>
          <input
            class="sr-only"
            type="file"
            multiple
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
      <div class="table-scroll min-h-0 flex-1">
        <table>
          <thead>
            <tr>
              <th>Path</th>
              <th>Version</th>
              <th>MIME</th>
              <th>Size</th>
              <th>Updated</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="loading">
              <td colspan="6" class="muted">Loading files…</td>
            </tr>
            <tr v-else-if="!files.length">
              <td colspan="6" class="muted">No reusable files yet.</td>
            </tr>
            <tr v-for="file in files" :key="file.descriptor.id">
              <td class="font-mono text-[12px]">{{ file.descriptor.path }}</td>
              <td>v{{ file.revision }}</td>
              <td>{{ file.descriptor.mime_type }}</td>
              <td>{{ formatBytes(file.descriptor.size_bytes) }}</td>
              <td>{{ formatDate(file.created_at) }}</td>
              <td class="whitespace-nowrap">
                <button class="btn btn-sm" type="button" @click="download(file)">
                  <Icon name="download" :size="13" /> Download
                </button>
                <button class="btn btn-sm btn-danger ml-1" type="button" @click="archive(file)">
                  <Icon name="trash" :size="13" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
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
import PanelHeader from "../components/shared/PanelHeader.vue";

const files = ref<WorkflowFile[]>([]);
const loading = ref(false);
const uploading = ref(0);
const error = ref("");

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

async function uploadFiles(event: Event) {
  const input = event.target as HTMLInputElement;
  const entries = [...(input.files ?? [])];
  input.value = "";

  if (!entries.length) {
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
