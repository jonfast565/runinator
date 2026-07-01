<template>
  <section class="pane artifacts-pane">
    <div class="panel">
      <div class="panel-toolbar">
        <h2>Artifacts</h2>
        <div class="btn-row">
          <button class="btn" :disabled="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
          <button class="btn btn-primary" :disabled="loading" @click="onUpload">
            <Icon name="upload" />
            <span>Upload</span>
          </button>
        </div>
      </div>
      <DataTable>
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Run</th>
              <th>Name</th>
              <th>MIME</th>
              <th>Size</th>
              <th>URI</th>
              <th>Created</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="artifact in store.artifacts"
              :key="artifact.id"
              :class="{ selected: store.selectedArtifactId === artifact.id }"
              @click="store.selectedArtifactId = artifact.id"
            >
              <td>{{ artifact.id }}</td>
              <td>{{ artifact.run_id }}</td>
              <td>{{ artifact.name }}</td>
              <td>{{ artifact.mime_type }}</td>
              <td>{{ formatBytes(artifact.size_bytes) }}</td>
              <td class="uri-cell">{{ artifact.uri }}</td>
              <td>{{ formatDate(artifact.created_at) }}</td>
              <td class="row-actions">
                <button class="btn btn-icon btn-ghost" title="Download" @click.stop="onDownload(artifact)">
                  <Icon name="download" />
                </button>
                <button class="btn btn-icon btn-ghost" title="Delete" @click.stop="onDelete(artifact)">
                  <Icon name="trash" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import { useArtifactsStore } from "../stores/artifacts";
import type { RunArtifact } from "../types/models";
import { formatDate } from "../utils/format";

const store = useArtifactsStore();
const loading = ref(false);

async function refresh() {
  loading.value = true;
  try {
    await store.refreshArtifacts();
  } finally {
    loading.value = false;
  }
}

async function onUpload() {
  await store.promptUploadArtifact();
}

async function onDownload(artifact: RunArtifact) {
  await store.promptDownloadArtifact(artifact);
}

async function onDelete(artifact: RunArtifact) {
  await store.removeArtifact(artifact);
}

function formatBytes(size: number): string {
  if (!Number.isFinite(size) || size <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = size;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const formatted = value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value).toString();
  return `${formatted} ${units[unit]}`;
}

onMounted(refresh);
</script>

<style scoped>
.uri-cell {
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  text-align: right;
}
</style>
