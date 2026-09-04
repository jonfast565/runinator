<template>
  <section class="pane h-full overflow-auto">
    <div class="panel min-h-full">
      <PanelHeader
        title="Workspaces"
        icon="folder"
        eyebrow="Durable results"
        description="Saved files and results, shared by key across workflows. Each version is immutable and compressed."
      >
        <button class="btn" :disabled="busy" @click="refresh">
          <Icon name="refresh" /> Refresh
        </button>
      </PanelHeader>
      <p v-if="error" role="alert" class="text-danger">{{ error }}</p>
      <LoadingPanel v-if="busy && !store.items.length" compact message="Loading workspaces…" />
      <div class="grid min-h-0 gap-4 lg:grid-cols-[minmax(240px,1fr)_minmax(0,2fr)]">
        <div>
          <EmptyState
            v-if="!filtered.length && !busy"
            icon="folder"
            title="No workspaces"
            description="Attach a keyed workspace to a workflow step to save files and results here."
          />
          <ul class="space-y-2">
            <li v-for="item in filtered" :key="item.id">
              <button
                class="w-full rounded border p-3 text-left"
                :class="{ 'bg-bg-muted': store.selected?.id === item.id }"
                :aria-pressed="store.selected?.id === item.id"
                @click="select(item)"
              >
                <strong class="block break-all">{{ item.key }}</strong>
                <span class="text-sm text-fg-muted"
                  >Version {{ item.head_version }} · {{ formatDate(item.updated_at) }}</span
                >
              </button>
            </li>
          </ul>
          <div class="btn-row mt-3">
            <button class="btn" :disabled="page === 0 || busy" @click="changePage(-1)">
              Previous
            </button>
            <span>Page {{ page + 1 }}</span>
            <button class="btn" :disabled="store.items.length < 50 || busy" @click="changePage(1)">
              Next
            </button>
          </div>
        </div>
        <div v-if="store.selected" class="min-w-0 space-y-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h2 class="break-all text-lg font-semibold">{{ store.selected.key }}</h2>
            <button
              class="btn btn-danger"
              :disabled="busy || store.selected.permission !== 'own'"
              @click="remove(null)"
            >
              <Icon name="trash" /> Delete workspace
            </button>
          </div>
          <p class="text-sm text-fg-muted">
            Pass this key with a version to reproduce a saved state. Active workspaces cannot be
            deleted.
          </p>
          <label class="block"
            >Saved version
            <select v-model="selectedVersion" class="mt-1 w-full rounded border p-2">
              <option
                v-for="version in store.versions"
                :key="version.version"
                :value="version.version"
              >
                v{{ version.version }} · {{ formatDate(version.created_at) }} ·
                {{ bytes(version.compressed_bytes) }} compressed
              </option>
            </select>
          </label>
          <div class="btn-row">
            <button
              class="btn"
              :disabled="versionPage === 0 || busy"
              @click="changeVersionPage(-1)"
            >
              Newer versions
            </button>
            <button
              class="btn"
              :disabled="store.versions.length < 50 || busy"
              @click="changeVersionPage(1)"
            >
              Older versions
            </button>
          </div>
          <template v-if="snapshot">
            <p class="break-all text-sm text-fg-muted">
              Run {{ snapshot.workflow_run_id }} · Attempt {{ snapshot.attempt }}
            </p>
            <div class="btn-row">
              <button class="btn btn-primary" :disabled="busy" @click="download()">
                <Icon name="download" /> Download archive
              </button>
              <button
                class="btn btn-danger"
                :disabled="
                  busy ||
                  snapshot.version === store.selected.head_version ||
                  !['edit', 'own'].includes(store.selected.permission)
                "
                @click="remove(snapshot.version)"
              >
                Delete version
              </button>
            </div>
            <h3 class="font-semibold">Saved results</h3>
            <pre class="max-h-72 overflow-auto rounded border p-3 text-xs">{{
              JSON.stringify(snapshot.results, null, 2)
            }}</pre>
            <h3 class="font-semibold">Files · {{ snapshot.files.length }}</h3>
            <div class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr>
                    <th class="text-left">Path</th>
                    <th>Size</th>
                    <th><span class="sr-only">Download</span></th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="file in snapshot.files" :key="file.path" class="border-t">
                    <td class="max-w-lg break-all py-2" :title="file.sha256">
                      {{ file.path
                      }}<span v-if="file.link_target" class="text-fg-muted">
                        → {{ file.link_target }}</span
                      >
                    </td>
                    <td class="whitespace-nowrap px-2">{{ bytes(file.size_bytes) }}</td>
                    <td>
                      <button
                        v-if="!file.link_target"
                        class="btn btn-sm"
                        :disabled="busy"
                        :aria-label="`Download ${file.path}`"
                        @click="download(file.path)"
                      >
                        <Icon name="download" />
                      </button>
                    </td>
                  </tr>
                  <tr v-if="!snapshot.files.length">
                    <td colspan="3" class="py-4 text-fg-muted">
                      This version contains saved results and no files.
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </template>
          <p v-else class="text-fg-muted">No committed versions yet.</p>
        </div>
        <EmptyState
          v-else
          icon="folder"
          title="Select a workspace"
          description="Inspect its saved versions, files, and results."
        />
      </div>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { DurableWorkspace } from "../../core/domain/models/workspaces";
import { formatDate } from "../../core/utils/format";
import { useWorkspacesStore } from "../adapters/pinia/workspaces";
import { useAppStore } from "../adapters/pinia/app";
import { downloadBlob } from "../adapters/browser/files";
import PanelHeader from "../components/shared/PanelHeader.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";

const store = useWorkspacesStore();
const app = useAppStore();
const busy = ref(false);
const error = ref("");
const page = ref(0);
const versionPage = ref(0);
const selectedVersion = ref<number | null>(null);
const filtered = computed(() =>
  store.items.filter((item) => item.key.toLowerCase().includes(app.normalizedSearch)),
);
const snapshot = computed(() =>
  store.versions.find((version) => version.version === selectedVersion.value),
);

function bytes(size: number) {
  return size < 1024
    ? `${String(size)} B`
    : size < 1048576
      ? `${(size / 1024).toFixed(1)} KiB`
      : `${(size / 1048576).toFixed(1)} MiB`;
}

async function operation(work: () => Promise<void>) {
  busy.value = true;
  error.value = "";

  try {
    await work();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function refresh() {
  await operation(() => store.refresh(page.value * 50));
}

async function select(item: DurableWorkspace) {
  versionPage.value = 0;
  await operation(async () => {
    await store.select(item);
    selectedVersion.value = store.versions[0]?.version ?? null;
  });
}

async function changePage(delta: number) {
  page.value += delta;
  await refresh();
}

async function changeVersionPage(delta: number) {
  versionPage.value += delta;
  await operation(async () => {
    await store.select(store.selected, versionPage.value * 50);
    selectedVersion.value = store.versions[0]?.version ?? null;
  });
}

async function download(path: string | null = null) {
  const selected = store.selected;
  const version = snapshot.value;

  if (!selected || !version) {
    return;
  }

  await operation(async () => {
    const blob = await store.download(selected.id, version.version, path);
    downloadBlob(path?.split("/").pop() ?? `workspace-v${String(version.version)}.tar.gz`, blob);
  });
}

async function remove(version: number | null) {
  const selected = store.selected;

  if (
    !selected ||
    !window.confirm(
      version === null
        ? `Delete workspace “${selected.key}” and all saved versions?`
        : `Delete version ${String(version)} permanently?`,
    )
  ) {
    return;
  }

  await operation(async () => {
    await store.remove(selected.id, version);
    await store.select(version === null ? null : selected);
    selectedVersion.value = store.versions[0]?.version ?? null;
    await store.refresh(page.value * 50);
  });
}

onMounted(refresh);
</script>
