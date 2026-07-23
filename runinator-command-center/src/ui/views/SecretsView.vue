<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      :storage-key="`command-center.settings.${settingKind}.split`"
      :initial-first-pct="60"
      :min-first="420"
      :min-second="320"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="!!secrets.selectedSecret"
    >
      <template #first>
        <div
          class="panel grid h-full min-h-0 gap-3 grid-rows-[auto_auto_1fr] max-[920px]:overflow-auto"
        >
          <header class="flex items-center justify-between gap-3">
            <div>
              <h2 class="m-0 text-base font-semibold text-fg">{{ pageTitle }}</h2>
              <p class="m-0 text-xs text-fg-muted">{{ pageDescription }}</p>
            </div>
            <div class="btn-row">
              <button class="btn" :disabled="loadingSecrets" @click="refreshPage">
                <LoadingSpinner v-if="loadingSecrets" size="sm" label="Refreshing secrets" />
                <Icon v-else name="refresh" />
                <span>Refresh</span>
              </button>
              <button class="btn btn-primary" @click="openNewSetting">
                <Icon name="plus" />
                <span>{{ newLabel }}</span>
              </button>
            </div>
          </header>

          <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
            <MetricCard
              :label="isConfig ? 'Configs' : 'Secrets'"
              :value="filteredEntries.length"
              value-class="text-base"
            />
            <MetricCard label="Scopes" :value="knownScopes.length" value-class="text-base" />
            <MetricCard
              label="Values"
              :value="isConfig ? 'Visible' : 'Hidden'"
              value-class="text-base"
            />
          </div>

          <div
            class="min-h-0 overflow-auto rounded-md border border-border-subtle bg-surface p-2"
          >
            <LoadingPanel
              v-if="loadingSecrets"
              compact
              :message="loadingSecretsMessage || 'Refreshing secrets…'"
            />
            <ul v-else-if="settingsTree.length" class="m-0 p-0">
              <SettingsTreeNode
                v-for="node in settingsTree"
                :key="node.path"
                :node="node"
                :is-config="isConfig"
                :config-values="secrets.configValues"
                :selected-key="secrets.selectedSecretKey"
                @select="selectOverview"
              />
            </ul>
            <div v-else class="px-6 py-6 text-center text-fg-muted">
              {{ isConfig ? "No configs match." : "No secrets match." }}
            </div>
          </div>
        </div>
      </template>
      <template #second>
        <div class="panel flex min-h-0 flex-col gap-3">
          <MobileBackBar @back="secrets.selectedSecretKey = ''" />
          <div class="flex items-center justify-between gap-3">
            <h2 class="m-0 text-base font-semibold text-fg">
              {{ isConfig ? "Config" : "Secret" }} Overview
            </h2>
            <div v-if="selected" class="btn-row">
              <button class="btn btn-sm" @click="openEditSetting(selected)">
                <Icon name="edit" :size="14" />
                <span>Edit</span>
              </button>
            </div>
          </div>
          <div v-if="!selected" class="py-3.5 text-fg-muted">
            Select an entry to preview its value. Editing happens in a modal.
          </div>
          <template v-else>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div>
                <label
                  class="mb-1 block text-[11px] tracking-wide text-fg-muted uppercase"
                  >Scope</label
                >
                <div>{{ selected.scope }}</div>
              </div>
              <div>
                <label
                  class="mb-1 block text-[11px] tracking-wide text-fg-muted uppercase"
                  >Name</label
                >
                <div>{{ selected.name }}</div>
              </div>
              <div>
                <label
                  class="mb-1 block text-[11px] tracking-wide text-fg-muted uppercase"
                  >Reference</label
                >
                <code class="break-words font-mono text-xs">{{
                  settingRef(selected.kind, selected.scope, selected.name)
                }}</code>
              </div>
              <div>
                <label
                  class="mb-1 block text-[11px] tracking-wide text-fg-muted uppercase"
                  >Kind</label
                >
                <div>{{ selected.kind }}</div>
              </div>
            </div>
            <div class="flex min-h-0 flex-1 flex-col">
              <label class="mb-1 text-[11px] tracking-wide text-fg-muted uppercase">Value</label>
              <pre
                v-if="isConfig"
                class="m-0 min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-surface-sunken p-3 font-mono text-xs"
                >{{ overviewValue }}</pre
              >
              <div
                v-else
                class="rounded-md border border-dashed border-border p-3 text-xs text-fg-muted"
              >
                Secret values are write-only and never displayed. Use Edit to replace it.
              </div>
            </div>
          </template>
        </div>
      </template>
    </SplitPane>

    <div
      v-if="editorOpen"
      ref="modalRoot"
      class="modal-backdrop"
      tabindex="-1"
      @keydown.esc.stop.prevent="closeEditor"
    >
      <form class="modal w-full max-w-[860px] [&_textarea]:min-h-[180px]" @submit.prevent="saveEditor">
        <header class="modal-header">
          <div>
            <h2 class="m-0">{{ formTitle }}</h2>
            <p class="m-0 text-xs text-fg-muted">{{ hint }}</p>
          </div>
          <button class="btn btn-ghost" type="button" @click="closeEditor">
            <Icon name="x" />
          </button>
        </header>

        <div class="form-grid !grid-cols-1 sm:!grid-cols-2">
          <label>
            <span>Scope</span>
            <input v-model="secrets.draft.scope" list="setting-scopes" placeholder="github" />
          </label>
          <label>
            <span>Name</span>
            <input v-model="secrets.draft.name" placeholder="token" />
          </label>
          <label class="col-span-full">
            <span>{{ isConfig ? "Config Value (JSON)" : "Secret Value" }}</span>
            <JsonEditor
              v-if="isConfig"
              class="min-h-[140px] [&_.json-editor-container]:min-h-24"
              :model-value="secrets.draft.secret"
              title=""
              @update:model-value="secrets.draft.secret = $event"
            />
            <textarea v-else v-model="secrets.draft.secret" :placeholder="valuePlaceholder" />
          </label>
        </div>
        <datalist id="setting-scopes">
          <option v-for="scope in knownScopes" :key="scope" :value="scope" />
        </datalist>
        <div
          class="grid gap-1.5 rounded-md border border-border-subtle bg-surface-subtle px-3 py-2.5"
        >
          <span class="text-xs text-fg-muted">Reference</span>
          <code class="break-words">{{
            settingRef(secrets.draft.kind, secrets.draft.scope, secrets.draft.name)
          }}</code>
        </div>

        <div class="modal-actions">
          <button
            class="btn btn-danger"
            type="button"
            :disabled="!secrets.selectedSecret"
            @click="deleteEditorSetting"
          >
            <Icon name="trash" />
            <span>Delete</span>
          </button>
          <button class="btn" type="button" @click="closeEditor">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />
            <span>{{ saveLabel }}</span>
          </button>
        </div>
      </form>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import JsonEditor from "../components/shared/JsonEditor.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import SettingsTreeNode from "../components/shared/SettingsTreeNode.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useProvidersStore } from "../../ui/adapters/pinia/providers";
import { useSecretsStore } from "../../ui/adapters/pinia/secrets";
import { useOperationLoading } from "../composables/useOperationLoading";
import type { CredentialSummary, SettingKind } from "../../core/domain/models";
import { secretKey, settingRef } from "../../core/utils/secrets";
import { buildSettingsTree } from "../../core/utils/settings-tree";

const props = defineProps<{
  settingKind: SettingKind;
}>();

const app = useAppStore();
const secrets = useSecretsStore();
const providers = useProvidersStore();
const { isLoading: loadingSecrets, loadingMessage: loadingSecretsMessage } =
  useOperationLoading(["Refreshing secrets", "Loading config value"]);
const editorOpen = ref(false);
const modalRoot = ref<HTMLElement | null>(null);

const isConfig = computed(() => props.settingKind === "config");
const baseEntries = computed(() =>
  isConfig.value ? secrets.configEntries : secrets.secretEntries,
);
const filteredEntries = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return baseEntries.value;
  }

  return baseEntries.value.filter((setting) =>
    [setting.scope, setting.name, settingRef(setting.kind, setting.scope, setting.name)]
      .join(" ")
      .toLowerCase()
      .includes(query),
  );
});
const settingsTree = computed(() => buildSettingsTree(filteredEntries.value));
const pageTitle = computed(() => (isConfig.value ? "Configs" : "Secrets"));
const pageDescription = computed(() =>
  isConfig.value
    ? "Plain JSON values resolved by the web service and visible to admins."
    : "Encrypted values resolved at the worker. Secret values are never displayed after saving.",
);
const newLabel = computed(() => (isConfig.value ? "New Config" : "New Secret"));
const formTitle = computed(
  () => `${secrets.selectedSecret ? "Update" : "Add"} ${isConfig.value ? "Config" : "Secret"}`,
);
const saveLabel = computed(() => (isConfig.value ? "Save Config" : "Save Secret"));
const valuePlaceholder = computed(() =>
  isConfig.value
    ? 'JSON value, e.g. "https://api.example.com" or { "url": "..." }'
    : "Paste the secret value to add or replace it.",
);
const hint = computed(() =>
  isConfig.value
    ? "Config values are visible JSON. The web service infers the slot schema from the first value; later writes must stay consistent with it."
    : "Secret values are write-only from this interface. Select an existing secret, enter a new value, and save to replace it.",
);

const knownScopes = computed(() => {
  const scopes = new Set<string>(secrets.scopes);

  for (const provider of providers.providers) {
    for (const scope of provider.metadata.credential_scopes) {
      scopes.add(scope);
    }
  }

  if (secrets.draft.scope.trim()) {
    scopes.add(secrets.draft.scope.trim());
  }

  return Array.from(scopes).sort();
});

async function refreshPage() {
  await secrets.refreshSecrets();

  if (isConfig.value) {
    await secrets.loadConfigValues(filteredEntries.value);
  }
}

function openNewSetting() {
  secrets.selectedSecretKey = "";
  secrets.clearDraft(props.settingKind);
  editorOpen.value = true;
}

// the currently previewed entry (right-hand overview pane).
const selected = computed<CredentialSummary | null>(() => secrets.selectedSecret);
const overviewValue = computed(() => {
  const setting = selected.value;

  if (!setting) {
    return "";
  }

  return secrets.configValues[secretKey(setting)] ?? "(no value loaded)";
});

// select an entry for the read-only overview pane without opening the editor modal.
async function selectOverview(setting: CredentialSummary) {
  secrets.selectSecret(setting);

  if (isConfig.value) {
    await secrets.loadConfigValue(setting);
  }
}

async function openEditSetting(setting: CredentialSummary) {
  secrets.selectSecret(setting);

  if (isConfig.value) {
    await secrets.loadConfigValue(setting);
    secrets.draft.secret = secrets.configValues[secretKey(setting)] ?? "";
  }

  editorOpen.value = true;
}

function closeEditor() {
  editorOpen.value = false;
}

async function saveEditor() {
  secrets.draft.kind = props.settingKind;
  await secrets.saveDraft();

  if (!app.errorText) {
    if (isConfig.value && secrets.selectedSecret) {
      await secrets.loadConfigValue(secrets.selectedSecret);
    }

    editorOpen.value = false;
  }
}

async function deleteEditorSetting() {
  const setting = secrets.selectedSecret;

  if (!setting) {
    return;
  }

  if (
    !window.confirm(
      `Delete ${isConfig.value ? "config" : "secret"} ${setting.scope}/${setting.name}?`,
    )
  ) {
    return;
  }

  await secrets.deleteSelectedSecret();

  if (!app.errorText) {
    editorOpen.value = false;
  }
}

watch(
  () => props.settingKind,
  () => {
    editorOpen.value = false;
    secrets.clearDraft(props.settingKind);
    void refreshPage();
  },
);

// focus the modal on open so its scoped escape handler works without a manual click.
watch(editorOpen, async (open) => {
  if (!open) {
    return;
  }

  await nextTick();
  modalRoot.value?.focus();
});

onMounted(() => {
  if (providers.providers.length === 0 && !providers.loading) {
    void providers.fetchProviders();
  }

  void refreshPage();
});
</script>

