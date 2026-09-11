<template>
  <section class="pane h-full overflow-hidden">
    <div class="panel h-full min-h-0">
      <PanelHeader
        title="Workspaces"
        icon="folder"
        eyebrow="Durable results"
        description="Inspect immutable workspace versions and recover the exact files and results produced by a workflow run."
      >
        <button class="btn" :disabled="busy" @click="refresh">
          <Icon name="refresh" />
          <span>{{ busy ? "Refreshing…" : "Refresh" }}</span>
        </button>
        <button class="btn btn-primary" :disabled="busy" @click="openImportDialog">
          <Icon name="upload" />
          <span>Import OCI archive</span>
        </button>
      </PanelHeader>

      <div v-if="error" class="workspace-error" role="alert">
        <Icon name="alert" />
        <span>{{ error }}</span>
        <button class="btn btn-sm btn-ghost ml-auto" type="button" @click="error = ''">
          Dismiss
        </button>
      </div>
      <Modal
        v-if="importDialogOpen"
        title="Import OCI workspace archive"
        description="Import an OCI workspace archive into an unused workspace key."
        width="min(520px, calc(100vw - 32px))"
        :close-on-backdrop="!busy"
        :close-on-esc="!busy"
        @close="closeImportDialog"
      >
        <form
          id="workspace-oci-import"
          class="workspace-import-form"
          @submit.prevent="importArchive"
        >
          <label>
            <span>Workspace key</span>
            <input v-model="importKey" placeholder="workspace-key" required autofocus />
          </label>
          <label v-if="!desktopRuntime">
            <span>OCI layout archive</span>
            <input type="file" accept=".tar" @change="chooseArchive" />
          </label>
          <p v-else class="workspace-import-hint">
            Choose the archive from the native file picker after starting the import.
          </p>
          <p v-if="error" class="workspace-import-error" role="alert">{{ error }}</p>
        </form>
        <template #actions>
          <button class="btn" type="button" :disabled="busy" @click="closeImportDialog">
            Cancel
          </button>
          <button
            class="btn btn-primary"
            type="submit"
            form="workspace-oci-import"
            :disabled="busy || !importKey.trim()"
          >
            Import archive
          </button>
        </template>
      </Modal>

      <div
        v-if="store.transfer && ['queued', 'running'].includes(store.transfer.state)"
        class="workspace-run-context"
        role="status"
      >
        <span
          >{{ store.transfer.importing ? "Import" : "Export" }} {{ store.transfer.state }} ·
          {{ bytes(store.transfer.bytes_processed) }} transferred</span
        >
        <button class="btn btn-sm" @click="store.cancelTransfer()">Cancel transfer</button>
      </div>
      <LoadingPanel v-if="busy && !store.items.length" compact message="Loading workspaces…" />

      <SplitPane
        v-else
        class="workspace-layout"
        storage-key="command-center.workspaces.split"
        :initial-first-pct="27"
        :min-first="280"
        :min-second="560"
        collapsible-first
        first-label="Saved workspaces"
        first-icon="folder"
      >
        <template #first>
          <aside class="workspace-browser" aria-label="Workspace browser">
            <div class="workspace-browser-heading">
              <div>
                <h2>Saved workspaces</h2>
                <p>
                  {{ filtered.length }} on page {{ page + 1 }}
                  <template v-if="app.normalizedSearch"> · filtered</template>
                </p>
              </div>
              <span class="badge status-muted">{{ store.items.length }}</span>
            </div>

            <EmptyState
              v-if="!filtered.length"
              compact
              :icon="store.items.length ? 'search' : 'folder'"
              :title="store.items.length ? 'No matches on this page' : 'No workspaces yet'"
              :description="
                store.items.length
                  ? `No workspace keys match “${app.searchQuery}”.`
                  : 'Attach a keyed workspace to a workflow step to save files and results here.'
              "
            />

            <ul v-else class="workspace-list">
              <li v-for="item in filtered" :key="item.id">
                <button
                  class="workspace-card"
                  :class="{ 'is-selected': store.selected?.id === item.id }"
                  :aria-pressed="store.selected?.id === item.id"
                  type="button"
                  @click="select(item)"
                >
                  <span class="workspace-card-icon"><Icon name="folder" :size="17" /></span>
                  <span class="workspace-card-copy">
                    <strong>{{ item.key }}</strong>
                    <span>Updated {{ formatDate(item.updated_at) }}</span>
                  </span>
                  <span class="workspace-card-meta">
                    <span class="workspace-version-pill">v{{ item.head_version }}</span>
                    <Icon name="chevron-right" :size="15" />
                  </span>
                </button>
              </li>
            </ul>

            <nav class="workspace-pagination" aria-label="Workspace pages">
              <button
                class="btn btn-sm btn-icon"
                type="button"
                aria-label="Previous workspace page"
                :disabled="page === 0 || busy"
                @click="changePage(-1)"
              >
                <Icon name="chevron-left" />
              </button>
              <span>Page {{ page + 1 }}</span>
              <button
                class="btn btn-sm btn-icon"
                type="button"
                aria-label="Next workspace page"
                :disabled="store.items.length < pageSize || busy"
                @click="changePage(1)"
              >
                <Icon name="chevron-right" />
              </button>
            </nav>
          </aside>
        </template>

        <template #second>
          <section v-if="store.selected" class="workspace-detail" aria-label="Workspace details">
            <header v-if="!snapshot" class="workspace-hero">
              <div class="workspace-hero-mark" aria-hidden="true">
                <Icon name="folder" :size="23" />
              </div>
              <div class="workspace-hero-copy">
                <div class="workspace-title-row">
                  <h2>{{ store.selected.key }}</h2>
                  <span class="badge status-muted">{{ permissionLabel }}</span>
                </div>
                <p>
                  Head v{{ store.selected.head_version }} · Updated
                  {{ formatDate(store.selected.updated_at) }}
                </p>
              </div>
              <div class="workspace-hero-actions">
                <button class="btn btn-sm" type="button" @click="copyWorkspaceKey">
                  <Icon :name="copyFeedback ? 'check' : 'copy'" :size="14" />
                  {{ copyFeedback || "Copy key" }}
                </button>
                <button
                  class="btn btn-sm btn-danger"
                  type="button"
                  :disabled="busy || !canDeleteWorkspace"
                  :title="
                    canDeleteWorkspace
                      ? 'Delete this workspace and every saved version'
                      : 'Only workspace owners can delete the workspace'
                  "
                  @click="remove(null)"
                >
                  <Icon name="trash" :size="14" /> Delete
                </button>
              </div>
            </header>

            <LoadingPanel
              v-if="busy && !store.versions.length"
              compact
              message="Loading workspace history…"
            />

            <SplitPane
              v-else-if="snapshot"
              class="workspace-version-split"
              orientation="vertical"
              storage-key="command-center.workspaces.version-detail"
              :initial-first-pct="32"
              :min-first="0"
              :min-second="160"
            >
              <template #first>
                <div class="workspace-version-summary">
                  <header class="workspace-hero">
                    <div class="workspace-hero-mark" aria-hidden="true">
                      <Icon name="folder" :size="23" />
                    </div>
                    <div class="workspace-hero-copy">
                      <div class="workspace-title-row">
                        <h2>{{ store.selected.key }}</h2>
                        <span class="badge status-muted">{{ permissionLabel }}</span>
                      </div>
                      <p>
                        Head v{{ store.selected.head_version }} · Updated
                        {{ formatDate(store.selected.updated_at) }}
                      </p>
                    </div>
                    <div class="workspace-hero-actions">
                      <button class="btn btn-sm" type="button" @click="copyWorkspaceKey">
                        <Icon :name="copyFeedback ? 'check' : 'copy'" :size="14" />
                        {{ copyFeedback || "Copy key" }}
                      </button>
                      <button
                        class="btn btn-sm btn-danger"
                        type="button"
                        :disabled="busy || !canDeleteWorkspace"
                        :title="
                          canDeleteWorkspace
                            ? 'Delete this workspace and every saved version'
                            : 'Only workspace owners can delete the workspace'
                        "
                        @click="remove(null)"
                      >
                        <Icon name="trash" :size="14" /> Delete
                      </button>
                    </div>
                  </header>
                  <section class="workspace-version-bar" aria-label="Selected workspace version">
                    <label>
                      <span>Saved version</span>
                      <select v-model="selectedVersion">
                        <option v-if="store.pinnedSnapshot" :value="store.pinnedSnapshot.version">
                          v{{ store.pinnedSnapshot.version }} · Pinned
                        </option>
                        <option
                          v-for="version in store.versions"
                          :key="version.version"
                          :value="version.version"
                        >
                          v{{ version.version }} · {{ formatDate(version.created_at) }}
                        </option>
                      </select>
                    </label>
                    <div class="workspace-version-nav">
                      <button
                        class="btn btn-sm"
                        type="button"
                        :disabled="versionPage === 0 || busy"
                        @click="changeVersionPage(-1)"
                      >
                        <Icon name="chevron-left" :size="14" /> Newer
                      </button>
                      <span>History page {{ versionPage + 1 }}</span>
                      <button
                        class="btn btn-sm"
                        type="button"
                        :disabled="store.versions.length < pageSize || busy"
                        @click="changeVersionPage(1)"
                      >
                        Older <Icon name="chevron-right" :size="14" />
                      </button>
                    </div>
                  </section>

                  <div class="workspace-metrics">
                    <MetricCard label="Entries" :value="snapshot.usage.entries" />
                    <MetricCard label="Logical size" :value="bytes(snapshot.usage.logical_bytes)" />
                    <MetricCard
                      label="Attempt"
                      :value="
                        snapshot.origin.kind === 'workflow' ? snapshot.origin.attempt : 'Imported'
                      "
                    />
                    <MetricCard
                      label="Parent version"
                      :value="`v${String(snapshot.parent_version)}`"
                    />
                  </div>

                  <section class="workspace-run-context">
                    <div>
                      <span>{{
                        snapshot.origin.kind === "workflow" ? "Produced by run" : "Import transfer"
                      }}</span>
                      <strong
                        :title="
                          snapshot.origin.kind === 'workflow'
                            ? snapshot.origin.workflow_run_id
                            : snapshot.origin.transfer_id
                        "
                        >{{
                          snapshot.origin.kind === "workflow"
                            ? snapshot.origin.workflow_run_id
                            : snapshot.origin.transfer_id
                        }}</strong
                      >
                    </div>
                    <div>
                      <span>Committed</span>
                      <strong>{{ formatDate(snapshot.created_at) }}</strong>
                    </div>
                    <div>
                      <span>Revision</span>
                      <strong :title="snapshot.revision_id">{{
                        shortHash(snapshot.revision_id)
                      }}</strong>
                    </div>
                    <div class="workspace-version-actions">
                      <button
                        class="btn btn-sm btn-primary"
                        type="button"
                        :disabled="busy"
                        @click="download()"
                      >
                        <Icon name="download" :size="14" /> Download archive
                      </button>
                      <button
                        class="btn btn-sm btn-ghost text-danger-fg"
                        type="button"
                        :disabled="busy || !canDeleteVersion"
                        :title="versionDeleteHint"
                        @click="remove(snapshot.version)"
                      >
                        <Icon name="trash" :size="14" /> Delete version
                      </button>
                    </div>
                  </section>

                  <section class="workspace-run-context">
                    <label
                      >Compare with
                      <select v-model="compareVersion">
                        <option :value="null">Choose version</option>
                        <option
                          v-for="version in store.versions"
                          :key="version.version"
                          :value="version.version"
                        >
                          v{{ version.version }}
                        </option>
                      </select>
                    </label>
                    <button
                      class="btn btn-sm"
                      :disabled="compareVersion === null || busy"
                      @click="compare()"
                    >
                      Compare
                    </button>
                    <button
                      class="btn btn-sm"
                      :disabled="!store.diff?.next_cursor || busy"
                      @click="compare(store.diff?.next_cursor ?? null)"
                    >
                      Next changes
                    </button>
                    <div v-if="store.diff">
                      <p v-for="(change, index) in store.diff.changes" :key="index">
                        {{
                          change.before === null
                            ? "Added"
                            : change.after === null
                              ? "Deleted"
                              : "Modified"
                        }}
                        {{ change.result ? "result: " : "" }}{{ change.path || "/" }}
                      </p>
                      <p v-if="!store.diff.changes.length">
                        {{
                          store.diff.next_cursor
                            ? "Continue to remaining changes."
                            : "No further changes."
                        }}
                      </p>
                    </div>
                  </section>
                </div>
              </template>
              <template #second>
                <div class="workspace-content">
                  <div
                    class="workspace-tabs"
                    role="tablist"
                    aria-label="Workspace version contents"
                  >
                    <button
                      id="workspace-files-tab"
                      type="button"
                      role="tab"
                      :aria-selected="activeTab === 'files'"
                      :class="{ 'is-active': activeTab === 'files' }"
                      @click="
                        activeTab = 'files';
                        store.clearPreview();
                      "
                    >
                      <Icon name="file" :size="15" /> Files
                      <span>{{ snapshot.usage.entries }}</span>
                    </button>
                    <button
                      id="workspace-results-tab"
                      type="button"
                      role="tab"
                      :aria-selected="activeTab === 'results'"
                      :class="{ 'is-active': activeTab === 'results' }"
                      @click="
                        activeTab = 'results';
                        store.clearPreview();
                      "
                    >
                      <Icon name="output" :size="15" /> Results
                      <span>{{ resultCount }}</span>
                    </button>
                  </div>

                  <div
                    v-if="activeTab === 'files'"
                    class="workspace-tab-panel"
                    role="tabpanel"
                    aria-labelledby="workspace-files-tab"
                  >
                    <WorkspaceFileBrowser
                      :key="snapshot.revision_id"
                      :directories="store.directories"
                      :path="directoryPath"
                      :busy="busy"
                      @open="openDirectory"
                      @expand="expandDirectory"
                      @collapse="store.collapseDirectory"
                      @preview="preview"
                      @download="download"
                    />
                    <pre v-if="store.preview" class="workspace-results">{{ store.preview }}</pre>
                  </div>

                  <div
                    v-else
                    class="workspace-tab-panel"
                    role="tabpanel"
                    aria-labelledby="workspace-results-tab"
                  >
                    <EmptyState
                      v-if="resultCount === 0"
                      compact
                      icon="output"
                      title="No saved results"
                      description="This version only contains files."
                    />
                    <div
                      v-for="result in store.results?.entries ?? []"
                      :key="result.name"
                      class="workspace-file-toolbar"
                    >
                      <span>{{ result.name }} · {{ bytes(result.size_bytes) }}</span>
                      <button
                        class="btn btn-sm"
                        :disabled="busy"
                        @click="preview(result.name, true)"
                      >
                        Preview
                      </button>
                      <button
                        class="btn btn-sm"
                        :disabled="busy"
                        @click="downloadResult(result.name)"
                      >
                        Download
                      </button>
                    </div>
                    <button
                      class="btn btn-sm"
                      :disabled="!store.results?.next_cursor || busy"
                      @click="loadResults(store.results?.next_cursor ?? null)"
                    >
                      Next results
                    </button>
                    <pre v-if="store.preview" class="workspace-results">{{ store.preview }}</pre>
                  </div>
                </div>
              </template>
            </SplitPane>

            <EmptyState
              v-else
              compact
              icon="folder"
              title="No committed versions"
              description="This workspace has not committed a durable snapshot yet."
            />
          </section>

          <EmptyState
            v-else
            class="workspace-detail-empty"
            icon="folder"
            title="Select a workspace"
            description="Choose a workspace to inspect its version history, files, and saved results."
          />
        </template>
      </SplitPane>
    </div>
  </section>
</template>

<script setup lang="ts">
import { isTauriRuntime } from "../../core/api/runtime";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DurableWorkspace } from "../../core/domain/models/workspaces";
import { formatDate } from "../../core/utils/format";
import { useWorkspacesStore } from "../adapters/pinia/workspaces";
import { useAppStore } from "../adapters/pinia/app";
import { downloadUrl } from "../adapters/browser/files";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import Modal from "../components/shared/Modal.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import WorkspaceFileBrowser from "../components/workspaces/WorkspaceFileBrowser.vue";
import SplitPane from "../components/shared/SplitPane.vue";

const pageSize = 50;
const store = useWorkspacesStore();
const app = useAppStore();
const importKey = ref("");
const importFile = ref<File | null>(null);
const importDialogOpen = ref(false);
const desktopRuntime = isTauriRuntime();

function chooseArchive(event: Event) {
  importFile.value = (event.target as HTMLInputElement).files?.[0] ?? null;
}

async function importArchive() {
  await operation(() => store.importArchive(importKey.value.trim(), importFile.value));

  if (error.value) {
    return;
  }

  closeImportDialog();
  await refresh();
}

function openImportDialog() {
  importKey.value = "";
  importFile.value = null;
  error.value = "";
  importDialogOpen.value = true;
}

function closeImportDialog() {
  if (busy.value) {
    return;
  }

  importDialogOpen.value = false;
}

const busy = ref(false);
const error = ref("");
const page = ref(0);
const versionPage = ref(0);
const selectedVersion = ref<number | null>(null);
const activeTab = ref<"files" | "results">("files");
const directoryPath = ref("");
const compareVersion = ref<number | null>(null);
const copyFeedback = ref("");
let copyReset: ReturnType<typeof setTimeout> | undefined;

const filtered = computed(() =>
  store.items.filter((item) => item.key.toLowerCase().includes(app.normalizedSearch)),
);
const snapshot = computed(
  () =>
    store.versions.find((version) => version.version === selectedVersion.value) ??
    (store.pinnedSnapshot?.version === selectedVersion.value ? store.pinnedSnapshot : null),
);
const resultCount = computed(() => store.results?.entries.length ?? 0);
const canDeleteWorkspace = computed(() => store.selected?.permission === "own");
const canDeleteVersion = computed(
  () =>
    Boolean(snapshot.value && store.selected) &&
    snapshot.value?.version !== store.selected?.head_version &&
    ["edit", "own"].includes(store.selected?.permission ?? "view"),
);
const permissionLabel = computed(() => {
  const labels = { view: "View only", run: "Run access", edit: "Can edit", own: "Owner" };
  return labels[store.selected?.permission ?? "view"];
});
const versionDeleteHint = computed(() => {
  if (snapshot.value?.version === store.selected?.head_version) {
    return "The current head version cannot be deleted";
  }

  return canDeleteVersion.value
    ? "Delete this immutable version"
    : "Edit or owner permission is required";
});

function bytes(size: number) {
  if (size < 1024) {
    return `${String(size)} B`;
  }

  if (size < 1048576) {
    return `${(size / 1024).toFixed(1)} KiB`;
  }

  if (size < 1073741824) {
    return `${(size / 1048576).toFixed(1)} MiB`;
  }

  return `${(size / 1073741824).toFixed(1)} GiB`;
}

function shortHash(value: string) {
  return value.length > 14 ? `${value.slice(0, 7)}…${value.slice(-7)}` : value;
}

async function operation(work: () => Promise<void>) {
  busy.value = true;
  error.value = "";

  try {
    await work();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    busy.value = false;
  }
}

function resetVersionView() {
  selectedVersion.value = store.versions[0]?.version ?? null;
  activeTab.value = "files";
  directoryPath.value = "";
}

async function refresh() {
  await operation(async () => {
    const selectedId = store.selected?.id;
    const pinned = selectedVersion.value;
    await store.refresh(page.value * pageSize);
    const next =
      store.items.find((item) => item.id === selectedId) ??
      (store.items.length ? store.items[0] : null);
    versionPage.value = 0;
    await store.select(next, 0, next?.id === selectedId ? pinned : null);
    resetVersionView();

    if (
      store.pinnedSnapshot?.version === pinned ||
      store.versions.some((item) => item.version === pinned)
    ) {
      selectedVersion.value = pinned;
    }
  });
}

async function select(item: DurableWorkspace) {
  if (store.selected?.id === item.id && store.versions.length) {
    return;
  }

  versionPage.value = 0;
  await operation(async () => {
    await store.select(item);
    resetVersionView();
  });
}

async function changePage(delta: number) {
  page.value = Math.max(0, page.value + delta);
  await refresh();
}

async function changeVersionPage(delta: number) {
  versionPage.value = Math.max(0, versionPage.value + delta);
  await operation(async () => {
    await store.select(store.selected, versionPage.value * pageSize);
    resetVersionView();
  });
}

async function copyWorkspaceKey() {
  const key = store.selected?.key;

  if (!key) {
    return;
  }

  try {
    await navigator.clipboard.writeText(key);
    copyFeedback.value = "Copied";
    clearTimeout(copyReset);
    copyReset = setTimeout(() => {
      copyFeedback.value = "";
    }, 1800);
  } catch {
    error.value = "Clipboard access failed. Select the workspace key and copy it manually.";
  }
}

async function download(path: string | null = null) {
  const selected = store.selected;
  const version = snapshot.value;

  if (!selected || !version) {
    return;
  }

  await operation(async () => {
    const url = await store.download(selected.id, version.version, path);

    if (!url) {
      return;
    }

    downloadUrl(
      path?.split("/").pop() ?? `${selected.key}-v${String(version.version)}.oci.tar`,
      url,
    );
  });
}

async function compare(cursor: string | null = null) {
  const selected = store.selected;
  const version = selectedVersion.value;
  const before = compareVersion.value;

  if (!selected || version === null || before === null) {
    return;
  }

  await operation(() => store.compare(selected.id, before, version, cursor));
}

async function openDirectory(path: string) {
  const selected = store.selected;
  const version = selectedVersion.value;

  if (!selected || version === null) {
    return;
  }

  directoryPath.value = path;
  store.clearPreview();
  await store.browse(selected.id, version, path);
}

async function expandDirectory(path: string) {
  const selected = store.selected;
  const version = selectedVersion.value;

  if (!selected || version === null) {
    return;
  }

  await store.expandDirectory(selected.id, version, path);
}

async function loadResults(cursor: string | null = null) {
  const selected = store.selected;
  const version = selectedVersion.value;

  if (!selected || version === null) {
    return;
  }

  await operation(() => store.browse(selected.id, version, "", cursor, true));
}

async function preview(path: string, result = false) {
  const selected = store.selected;
  const version = selectedVersion.value;

  if (!selected || version === null) {
    return;
  }

  await operation(() => store.previewFile(selected.id, version, path, result));
}

async function downloadResult(name: string) {
  const selected = store.selected;
  const version = selectedVersion.value;

  if (!selected || version === null) {
    return;
  }

  await operation(async () => {
    const url = await store.download(selected.id, version, name, true);

    if (url) {
      downloadUrl(`${name}.json`, url);
    }
  });
}

watch(
  () => snapshot.value?.revision_id,
  async (revision) => {
    if (revision) {
      void openDirectory("");

      if (activeTab.value === "results") {
        await loadResults();
      }
    }
  },
);

watch(activeTab, async (tab) => {
  if (tab === "results" && !store.results) {
    await loadResults();
  }
});

async function remove(version: number | null) {
  const selected = store.selected;

  if (
    !selected ||
    !window.confirm(
      version === null
        ? `Delete workspace “${selected.key}” and all saved versions? This cannot be undone.`
        : `Delete version ${String(version)} permanently? This cannot be undone.`,
    )
  ) {
    return;
  }

  await operation(async () => {
    await store.remove(selected.id, version);

    if (version === null) {
      await store.refresh(page.value * pageSize);
      await store.select(store.items.length ? store.items[0] : null);
    } else {
      await store.select(selected, versionPage.value * pageSize);
    }

    resetVersionView();
  });
}

onMounted(refresh);
onBeforeUnmount(() => {
  clearTimeout(copyReset);
});
</script>

<style scoped>
.workspace-layout {
  min-height: 0;
  flex: 1;
  overflow: hidden;
}

.workspace-error {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  border-radius: var(--radius);
  background: var(--danger-bg);
  padding: 8px 10px;
  color: var(--danger-fg);
  font-size: 12px;
}

.workspace-import-form {
  display: grid;
  gap: 12px;
}

.workspace-import-form label {
  display: grid;
  gap: 5px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 650;
}

.workspace-import-form input {
  width: 100%;
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 8px 9px;
  color: var(--text);
}

.workspace-import-hint {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.workspace-import-error {
  margin: 0;
  color: var(--danger-fg);
  font-size: 12px;
}

.workspace-browser,
.workspace-detail {
  width: 100%;
  min-width: 0;
  min-height: 0;
  flex: 1 1 auto;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  background: var(--surface-subtle);
}

.workspace-browser {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.workspace-browser-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border-bottom: 1px solid var(--border-subtle);
  padding: 12px;
}

.workspace-browser-heading h2,
.workspace-hero h2 {
  margin: 0;
  color: var(--text);
}

.workspace-browser-heading h2 {
  font-size: 13px;
  font-weight: 700;
}

.workspace-browser-heading p,
.workspace-hero p {
  margin: 2px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}

.workspace-list {
  min-height: 0;
  flex: 1;
  margin: 0;
  padding: 6px;
  overflow: auto;
  list-style: none;
}

.workspace-list li + li {
  margin-top: 3px;
}

.workspace-card {
  position: relative;
  display: grid;
  width: 100%;
  grid-template-columns: 32px minmax(0, 1fr) auto;
  align-items: center;
  gap: 9px;
  border: 1px solid transparent;
  border-radius: var(--radius);
  background: transparent;
  padding: 9px;
  color: var(--text);
  text-align: left;
  transition:
    background 150ms ease,
    border-color 150ms ease,
    box-shadow 150ms ease;
}

.workspace-card:hover {
  border-color: var(--border-subtle);
  background: var(--surface-hover);
}

.workspace-card.is-selected {
  border-color: color-mix(in srgb, var(--accent) 34%, var(--border));
  background: var(--accent-soft);
  box-shadow: inset 3px 0 0 var(--accent);
}

.workspace-card-icon,
.workspace-hero-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border));
  background: var(--surface);
  color: var(--accent-text);
}

.workspace-card-icon {
  width: 32px;
  height: 32px;
  border-radius: 8px;
}

.workspace-card-copy {
  min-width: 0;
}

.workspace-card-copy strong,
.workspace-card-copy span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-card-copy strong {
  font-family: var(--font-mono);
  font-size: 12px;
}

.workspace-card-copy span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 10px;
}

.workspace-card-meta {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--text-muted);
}

.workspace-version-pill {
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 2px 6px;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
}

.workspace-card.is-selected .workspace-version-pill {
  background: var(--surface);
  color: var(--accent-text);
}

.workspace-pagination {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 9px;
  border-top: 1px solid var(--border-subtle);
  padding: 8px;
  color: var(--text-muted);
  font-size: 11px;
}

.workspace-detail {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 12px;
  overflow: hidden;
}

.workspace-hero {
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) auto;
  align-items: center;
  gap: 11px;
}

.workspace-hero-mark {
  width: 44px;
  height: 44px;
  border-radius: 11px;
  background: var(--accent-soft);
}

.workspace-hero-copy,
.workspace-title-row {
  min-width: 0;
}

.workspace-title-row,
.workspace-hero-actions,
.workspace-version-actions,
.workspace-version-nav {
  display: flex;
  align-items: center;
  gap: 6px;
}

.workspace-title-row h2 {
  overflow: hidden;
  font-family: var(--font-mono);
  font-size: 16px;
  font-weight: 700;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-hero-actions {
  flex-wrap: wrap;
  justify-content: flex-end;
}

.workspace-version-bar {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 10px;
  border-top: 1px solid var(--border-subtle);
  padding-top: 12px;
}

.workspace-version-bar label {
  display: grid;
  min-width: min(340px, 100%);
  gap: 4px;
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.workspace-version-bar select {
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 7px 9px;
  color: var(--text);
  font-size: 12px;
  text-transform: none;
}

.workspace-version-nav {
  color: var(--text-muted);
  font-size: 10px;
}

.workspace-version-split {
  min-height: 0;
  flex: 1 1 auto;
}

.workspace-version-summary {
  display: flex;
  min-height: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 12px;
  overflow: auto;
}

.workspace-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 7px;
}

.workspace-run-context {
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) auto auto auto;
  align-items: end;
  gap: 12px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 9px 10px;
}

.workspace-run-context > div:not(.workspace-version-actions) {
  min-width: 0;
}

.workspace-run-context span,
.workspace-run-context strong {
  display: block;
}

.workspace-run-context span {
  margin-bottom: 3px;
  color: var(--text-muted);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}

.workspace-run-context strong {
  overflow: hidden;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-content {
  display: flex;
  height: 100%;
  min-height: 0;
  min-width: 0;
  flex-direction: column;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  background: var(--surface);
  overflow: hidden;
}

.workspace-tabs {
  display: flex;
  gap: 3px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
  padding: 5px 6px 0;
}

.workspace-tabs button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid transparent;
  border-bottom: 0;
  border-radius: var(--radius) var(--radius) 0 0;
  background: transparent;
  padding: 7px 10px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 650;
}

.workspace-tabs button:hover {
  color: var(--text);
}

.workspace-tabs button.is-active {
  border-color: var(--border-subtle);
  background: var(--surface);
  color: var(--accent-text);
  box-shadow: 0 1px 0 var(--surface);
}

.workspace-tabs button span {
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 1px 5px;
  color: var(--text-muted);
  font-size: 9px;
}

.workspace-tab-panel {
  display: flex;
  min-height: 0;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 8px;
  padding: 8px;
  overflow: hidden;
}

.workspace-file-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 8px;
}

.workspace-file-toolbar label {
  display: flex;
  width: min(360px, 100%);
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: 0 8px;
  color: var(--text-muted);
}

.workspace-file-toolbar input {
  min-width: 0;
  flex: 1;
  border: 0;
  background: transparent;
  padding: 7px 0;
  color: var(--text);
  font-size: 11px;
  outline: none;
}

.workspace-file-toolbar > span {
  color: var(--text-muted);
  font-size: 10px;
}

.workspace-files-table-wrap {
  max-height: 380px;
  overflow: auto;
}

.workspace-files-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.workspace-files-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--surface);
  color: var(--text-muted);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.05em;
  text-align: left;
  text-transform: uppercase;
}

.workspace-files-table th,
.workspace-files-table td {
  border-bottom: 1px solid var(--border-faint);
  padding: 7px 8px;
}

.workspace-files-table th:not(:first-child),
.workspace-files-table td:not(:first-child) {
  white-space: nowrap;
}

.workspace-file-path {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
  color: var(--text-muted);
}

.workspace-file-path > span {
  min-width: 0;
}

.workspace-file-path strong,
.workspace-file-path small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-file-path strong {
  max-width: 52ch;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 600;
}

.workspace-file-path small {
  max-width: 60ch;
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 9px;
}

.workspace-files-table code {
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: 9px;
}

.workspace-results {
  max-height: 420px;
  margin: 0;
  border-radius: var(--radius);
  background: var(--surface-sunken);
  padding: 12px;
  overflow: auto;
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 1.6;
}

.workspace-detail-empty {
  width: 100%;
  min-height: 260px;
  flex: 1 1 auto;
  border: 1px dashed var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-subtle);
}

@media (max-width: 1100px) {
  .workspace-run-context {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .workspace-version-actions {
    justify-content: flex-start;
  }
}

@media (max-width: 1024px) {
  .workspace-layout > :deep(.split-section) {
    flex: 0 0 auto;
    overflow: visible;
  }

  .workspace-layout {
    grid-template-columns: 1fr;
    overflow: auto;
  }

  .workspace-browser {
    max-height: 280px;
  }

  .workspace-detail {
    overflow: visible;
  }

  .workspace-version-summary,
  .workspace-tab-panel {
    overflow: visible;
  }

  .workspace-hero {
    grid-template-columns: 38px minmax(0, 1fr);
  }

  .workspace-hero-mark {
    width: 38px;
    height: 38px;
  }

  .workspace-hero-actions {
    grid-column: 1 / -1;
    justify-content: flex-start;
  }

  .workspace-version-bar {
    align-items: stretch;
    flex-direction: column;
  }

  .workspace-version-nav {
    justify-content: space-between;
  }

  .workspace-metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .workspace-run-context {
    grid-template-columns: 1fr;
  }

  .workspace-file-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .workspace-files-table th:nth-child(3),
  .workspace-files-table td:nth-child(3) {
    display: none;
  }
}
</style>
