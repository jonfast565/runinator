<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.functions.split"
      :initial-first-pct="46"
      :min-first="360"
      :min-second="420"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="!!functions.selectedPackage"
    >
      <template #first>
        <div class="panel">
          <PanelHeader
            title="Functions"
            description="A packaged function is immutable code published to the platform and called like any other action. Publish uploads a built archive and its manifest; runinatorctl functions publish &lt;path&gt; builds that archive from a working tree. Versions never change, and only aliases move."
          >
            <div class="btn-row">
              <button class="btn" :disabled="loading" @click="functions.refreshPackages">
                <LoadingSpinner v-if="loading" size="sm" label="Refreshing functions" />
                <Icon v-else name="refresh" />
                <span>Refresh</span>
              </button>
              <button class="btn btn-primary" :disabled="!canManage" @click="publishing = true">
                <Icon name="upload" />
                <span>Publish</span>
              </button>
              <button
                class="btn btn-danger"
                :disabled="!functions.selectedPackage || !canManage"
                @click="removeSelected"
              >
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
          </PanelHeader>
          <DataTable>
            <thead>
              <tr>
                <th>Package</th>
                <th>Latest</th>
                <th>Description</th>
              </tr>
            </thead>
            <tbody>
              <tr v-if="loading && !functions.packages.length">
                <td colspan="3" class="px-3.5 py-3.5 text-center text-fg-muted">
                  <LoadingPanel compact :message="loadingMessage || 'Refreshing functions…'" />
                </td>
              </tr>
              <tr v-else-if="!functions.filteredPackages.length">
                <td colspan="3" class="!p-0 hover:!bg-transparent">
                  <EmptyState
                    class="functions-empty-state"
                    compact
                    :icon="functions.packages.length ? 'search' : 'box'"
                    :title="functions.packages.length ? 'No matches' : 'No functions published'"
                    :description="
                      functions.packages.length
                        ? `No packages match “${app.searchQuery}”.`
                        : 'Publish a package directory with `runinatorctl functions publish <path>` to call it from a workflow.'
                    "
                  />
                </td>
              </tr>
              <tr
                v-for="pkg in functions.filteredPackages"
                :key="pkg.id"
                class="cursor-pointer"
                :class="{ selected: functions.selectedPackage?.id === pkg.id }"
                @click="functions.selectPackage(pkg)"
              >
                <td class="font-mono text-[12px]">{{ qualifiedPackageName(pkg) }}</td>
                <td>{{ pkg.latest_version ?? "—" }}</td>
                <td>{{ pkg.description ?? "" }}</td>
              </tr>
            </tbody>
          </DataTable>
        </div>
      </template>

      <template #second>
        <div class="panel details overflow-auto">
          <MobileBackBar @back="functions.selectPackage(null)" />
          <EmptyState
            v-if="!functions.selectedPackage"
            icon="box"
            title="No package selected"
            description="Pick a package to see its versions, aliases, and exports."
          />
          <template v-else>
            <h2 class="m-0 text-base font-semibold text-fg">
              {{ qualifiedPackageName(functions.selectedPackage) }}
            </h2>
            <p v-if="functions.selectedPackage.description" class="hint m-0">
              {{ functions.selectedPackage.description }}
            </p>

            <div class="mt-2 flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">Aliases</h3>
              <HelpBubble
                text="An alias is the only mutable part of a published package. Moving one changes what new calls resolve to; an already-compiled workflow keeps calling the exact version it recorded."
                label="About function aliases"
              />
            </div>
            <DataTable>
              <thead>
                <tr>
                  <th>Alias</th>
                  <th>Version</th>
                  <th class="w-px"></th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="!aliases.length">
                  <td colspan="3" class="px-3.5 py-2 text-fg-muted">
                    No aliases. Publishing moves <code>latest</code> unless the manifest opts out.
                  </td>
                </tr>
                <tr v-for="alias in aliases" :key="alias.id">
                  <td class="font-mono text-[12px]">{{ alias.name }}</td>
                  <td>{{ alias.version }}</td>
                  <td>
                    <button
                      class="btn btn-sm"
                      :disabled="!canManage"
                      @click="removeAlias(alias.name)"
                    >
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              </tbody>
            </DataTable>

            <div class="btn-row items-end">
              <label class="grid gap-1 text-xs text-fg-muted">
                Alias
                <input
                  v-model.trim="promoteAlias"
                  required
                  maxlength="64"
                  pattern="[A-Za-z][A-Za-z0-9_-]*"
                  title="Start with a letter and use only letters, numbers, underscores, and hyphens."
                  placeholder="production"
                />
              </label>
              <label class="grid gap-1 text-xs text-fg-muted">
                Version
                <select v-model.number="promoteVersion">
                  <option v-for="version in versions" :key="version.id" :value="version.version">
                    {{ version.version }}
                  </option>
                </select>
              </label>
              <button class="btn btn-primary" :disabled="!canPromote" @click="promote">
                <Icon name="approve" />
                <span>Move alias</span>
              </button>
            </div>
            <p v-if="aliasError" class="error m-0 text-xs" role="alert">{{ aliasError }}</p>

            <div class="mt-2 flex items-center gap-1">
              <h3 class="m-0 text-sm font-semibold text-fg">Exports</h3>
              <HelpBubble
                text="Call an export from a workflow with the dotted path below. An unversioned call pins the newest version at compile time."
                label="About function exports"
              />
            </div>
            <DataTable>
              <thead>
                <tr>
                  <th>Call</th>
                  <th>Version</th>
                  <th>Aliases</th>
                  <th>Digest</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="!functions.selectedExports.length">
                  <td colspan="4" class="px-3.5 py-2 text-fg-muted">No exports published.</td>
                </tr>
                <tr v-for="entry in functions.selectedExports" :key="entry.export_id">
                  <td class="font-mono text-[12px]">{{ functionCallPath(entry) }}</td>
                  <td>{{ entry.version }}</td>
                  <td class="font-mono text-[11px]">{{ (entry.aliases ?? []).join(", ") }}</td>
                  <td class="font-mono text-[11px]" :title="entry.artifact_digest">
                    {{ shortDigest(entry.artifact_digest) }}
                  </td>
                </tr>
              </tbody>
            </DataTable>

            <h3 class="m-0 mt-2 text-sm font-semibold text-fg">Versions</h3>
            <DataTable>
              <thead>
                <tr>
                  <th>Version</th>
                  <th>Runtime</th>
                  <th>Digest</th>
                  <th>Published</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="version in versions" :key="version.id">
                  <td>{{ version.version }}</td>
                  <td class="font-mono text-[12px]">{{ version.runtime?.runtime ?? "" }}</td>
                  <td class="font-mono text-[11px]" :title="version.artifact_digest">
                    {{ shortDigest(version.artifact_digest) }}
                  </td>
                  <td>{{ version.created_at }}</td>
                </tr>
              </tbody>
            </DataTable>
          </template>
        </div>
      </template>
    </SplitPane>
    <PublishFunctionDialog v-if="publishing" @close="publishing = false" />
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import PublishFunctionDialog from "../components/functions/PublishFunctionDialog.vue";
import { useFunctionsStore } from "../adapters/pinia/functions";
import { useOrgsStore } from "../adapters/pinia/orgs";
import { useAppStore } from "../adapters/pinia/app";
import { useOperationLoading } from "../composables/useOperationLoading";
import { useCan } from "../composables/useCan";
import { functionCallPath, qualifiedPackageName, shortDigest } from "../../core/domain/models";

const functions = useFunctionsStore();
const orgs = useOrgsStore();
const app = useAppStore();
const { isLoading: loading, loadingMessage } = useOperationLoading("Refreshing functions");
// The backend enforces this. Disabling the controls only prevents the UI from offering an action that
// would be refused.
const { can } = useCan();
const canManage = computed(() => can("functions:manage"));

const publishing = ref(false);
const promoteAlias = ref("production");
const promoteVersion = ref<number | null>(null);

const aliases = computed(() => functions.selectedPackage?.aliases ?? []);
const versions = computed(() =>
  [...(functions.selectedPackage?.versions ?? [])].sort(
    (left, right) => right.version - left.version,
  ),
);
const aliasError = computed(() => {
  const value = promoteAlias.value.trim();

  if (!value) {
    return "Alias is required.";
  }

  return /^[A-Za-z][A-Za-z0-9_-]*$/.test(value)
    ? ""
    : "Alias must start with a letter and contain only letters, numbers, underscores, and hyphens.";
});
const canPromote = computed(
  () => canManage.value && !aliasError.value && promoteVersion.value !== null,
);

async function removeSelected() {
  const selected = functions.selectedPackage;

  if (
    !selected ||
    !window.confirm(`Delete function package “${qualifiedPackageName(selected)}”?`)
  ) {
    return;
  }

  await functions.removeSelected();
}

async function removeAlias(name: string) {
  if (
    !window.confirm(`Remove alias “${name}”? Already compiled calls keep their pinned version.`)
  ) {
    return;
  }

  await functions.removeAlias(name);
}

// default the version picker to the newest, which is what a promotion almost always means.
watch(versions, (list) => {
  promoteVersion.value = list.at(0)?.version ?? null;
});

async function promote() {
  if (promoteVersion.value === null) {
    return;
  }

  await functions.promote(promoteAlias.value.trim(), promoteVersion.value);
}

async function refresh() {
  functions.clearFunctions();
  await functions.refreshPackages();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
