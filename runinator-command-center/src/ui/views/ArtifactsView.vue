<template>
  <section class="pane">
    <div class="panel">
      <PanelHeader title="Artifacts">
        <Button variant="default" :loading="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
        <button class="btn btn-primary" :disabled="loading" @click="onUpload">
          <Icon name="upload" />
          <span>Upload</span>
        </button>
      </PanelHeader>
      <DataTable
        :columns="columns"
        :rows="filteredArtifacts"
        row-key="id"
        :selected-key="store.selectedArtifactId"
        :loading="loading"
        loading-message="Loading artifacts…"
        responsive="cards"
        :empty-title="store.artifacts.length ? 'No matches' : 'No artifacts yet'"
        :empty-description="
          store.artifacts.length
            ? `No artifacts match “${app.searchQuery}”.`
            : 'Uploaded and run-generated artifacts appear here.'
        "
        empty-icon="box"
        @select="store.selectedArtifactId = $event.id"
      >
        <template #cell-size_bytes="{ row }">{{ formatBytes(row.size_bytes) }}</template>
        <template #cell-uri="{ value }">
          <span class="max-w-80 truncate">{{ value }}</span>
        </template>
        <template #cell-created_at="{ row }">{{ formatDate(row.created_at) }}</template>
        <template #cell-actions="{ row }">
          <span class="text-right">
            <button class="btn btn-icon btn-ghost" title="Download" @click.stop="onDownload(row)">
              <Icon name="download" />
            </button>
            <button class="btn btn-icon btn-ghost" title="Delete" @click.stop="onDelete(row)">
              <Icon name="trash" />
            </button>
          </span>
        </template>
      </DataTable>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import DataTable, { type DataTableColumn } from "../components/shared/DataTable.vue";
import Button from "../components/shared/Button.vue";
import Icon from "../components/shared/Icon.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import { useArtifactsStore } from "../../ui/adapters/pinia/artifacts";
import { useAppStore } from "../../ui/adapters/pinia/app";
import type { RunArtifact } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";

// low-priority columns collapse on mobile scroll mode; on phones the table becomes cards instead.
const columns: DataTableColumn<RunArtifact>[] = [
  { key: "id", label: "ID", priority: "low" },
  { key: "run_id", label: "Run" },
  { key: "name", label: "Name" },
  { key: "mime_type", label: "MIME", priority: "low" },
  { key: "size_bytes", label: "Size" },
  { key: "uri", label: "URI", priority: "low" },
  { key: "created_at", label: "Created", priority: "low" },
  { key: "actions", label: "Actions", align: "right" },
];

const store = useArtifactsStore();
const app = useAppStore();
const loading = ref(false);

// filter artifacts by the global search box (matches name, run id, mime type, or uri).
const filteredArtifacts = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return store.artifacts;
  }

  return store.artifacts.filter((artifact) =>
    [artifact.name, artifact.run_id, artifact.mime_type, artifact.uri].some((value) =>
      value.toLowerCase().includes(query),
    ),
  );
});

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
  if (!Number.isFinite(size) || size <= 0) {
    return "0 B";
  }

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
