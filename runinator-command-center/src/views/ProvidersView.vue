<template>
  <section class="pane providers-pane">
    <div class="providers-layout">
      <aside class="panel providers-list-panel">
        <div class="panel-toolbar">
          <h2>Providers</h2>
          <button class="btn" :disabled="providers.loading" @click="providers.fetchProviders()">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
        <div v-if="providers.error" class="providers-error">{{ providers.error }}</div>
        <div v-if="!providers.providers.length" class="empty-state">No providers registered.</div>
        <div v-else-if="!filteredProviders.length" class="empty-state">
          No providers match "{{ app.searchQuery }}".
        </div>
        <div class="providers-tree">
          <div v-for="provider in filteredProviders" :key="provider.name" class="providers-group">
            <button
              type="button"
              class="providers-group-head"
              :class="{ selected: selectedProvider === provider.name && !selectedAction }"
              @click="selectProvider(provider.name)"
            >
              <Icon name="box" :size="14" />
              <span class="providers-group-name">{{ provider.name }}</span>
              <span class="providers-group-count">{{ provider.actions.length }}</span>
            </button>
            <button
              v-for="action in provider.actions"
              :key="action.function_name"
              type="button"
              class="providers-action"
              :class="{
                selected:
                  selectedProvider === provider.name && selectedAction === action.function_name,
              }"
              @click="selectAction(provider.name, action.function_name)"
            >
              {{ action.function_name }}
            </button>
          </div>
        </div>
      </aside>

      <section class="panel providers-detail-panel">
        <template v-if="currentAction && currentProvider">
          <div class="providers-detail-head">
            <h2>{{ currentProvider.name }}.{{ currentAction.function_name }}</h2>
          </div>
          <p v-if="currentAction.description" class="providers-detail-desc">
            {{ currentAction.description }}
          </p>
          <div
            v-if="
              currentProvider.metadata.credential_scopes.length || currentProvider.metadata.contract
            "
            class="providers-meta-row"
          >
            <span
              v-for="scope in currentProvider.metadata.credential_scopes"
              :key="scope"
              class="providers-chip"
            >
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
            <span v-if="currentProvider.metadata.contract" class="providers-chip muted">{{
              currentProvider.metadata.contract
            }}</span>
          </div>

          <h3 class="providers-section-title">Parameters</h3>
          <div v-if="!currentAction.parameters.length" class="providers-none">No parameters.</div>
          <div v-else class="table-scroll compact-scroll">
            <table class="compact">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Type</th>
                  <th>Req</th>
                  <th>Default</th>
                  <th>Description</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="param in currentAction.parameters" :key="param.name">
                  <td>
                    <span class="providers-param-name">{{ param.name }}</span>
                    <span v-if="param.secret" class="providers-tag secret">secret</span>
                  </td>
                  <td>
                    <code>{{ describeType(param.ty) }}</code>
                  </td>
                  <td>
                    <span :class="['providers-tag', param.required ? 'req' : 'opt']">{{
                      param.required ? "yes" : "no"
                    }}</span>
                  </td>
                  <td>
                    <code
                      v-if="param.default_value !== undefined && param.default_value !== null"
                      >{{ shortJson(param.default_value) }}</code
                    ><span v-else class="providers-dim">—</span>
                  </td>
                  <td>{{ param.description || param.label || "" }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <h3 class="providers-section-title">Results</h3>
          <div v-if="!currentAction.results.length" class="providers-none">
            No declared results.
          </div>
          <div v-else class="table-scroll compact-scroll">
            <table class="compact">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Type</th>
                  <th>Description</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="result in currentAction.results" :key="result.name">
                  <td>
                    <span class="providers-param-name">{{ result.name }}</span>
                  </td>
                  <td>
                    <code>{{ describeType(result.ty) }}</code>
                  </td>
                  <td>{{ result.description || result.label || "" }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </template>

        <template v-else-if="currentProvider">
          <div class="providers-detail-head">
            <h2>{{ currentProvider.name }}</h2>
          </div>
          <div v-if="currentProvider.metadata.credential_scopes.length" class="providers-meta-row">
            <span
              v-for="scope in currentProvider.metadata.credential_scopes"
              :key="scope"
              class="providers-chip"
            >
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
          </div>
          <h3 class="providers-section-title">Actions</h3>
          <ul class="providers-action-summary">
            <li
              v-for="action in currentProvider.actions"
              :key="action.function_name"
              @click="selectAction(currentProvider.name, action.function_name)"
            >
              <span class="providers-param-name">{{ action.function_name }}</span>
              <span class="providers-dim">{{ action.description || "" }}</span>
            </li>
          </ul>
        </template>

        <div v-else class="empty-state">Select a provider to view its actions.</div>
      </section>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import { useProvidersStore } from "../stores/providers";
import { useAppStore } from "../stores/app";
import type { RuninatorType } from "../types/models";

const providers = useProvidersStore();
const app = useAppStore();
const selectedProvider = ref("");
const selectedAction = ref("");

// filter the provider tree by the global search box (matches provider or action name).
const filteredProviders = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return providers.providers;
  }

  return providers.providers.filter(
    (provider) =>
      provider.name.toLowerCase().includes(query) ||
      provider.actions.some((action) => action.function_name.toLowerCase().includes(query)),
  );
});

const currentProvider = computed(
  () => providers.providers.find((provider) => provider.name === selectedProvider.value) ?? null,
);
const currentAction = computed(() => {
  if (!selectedAction.value) {
    return null;
  }

  return (
    currentProvider.value?.actions.find(
      (action) => action.function_name === selectedAction.value,
    ) ?? null
  );
});

function selectProvider(name: string) {
  selectedProvider.value = name;
  selectedAction.value = "";
}

function selectAction(provider: string, action: string) {
  selectedProvider.value = provider;
  selectedAction.value = action;
}

// apply a deep-linked focus, falling back to the first provider when nothing is selected.
function applyFocus() {
  if (providers.focusedProvider) {
    selectedProvider.value = providers.focusedProvider;
    selectedAction.value = providers.focusedAction;
    providers.focusProviderAction("", "");
    return;
  }

  if (!selectedProvider.value && providers.providers.length) {
    selectedProvider.value = providers.providers[0].name;
  }
}

onMounted(async () => {
  if (!providers.providers.length) {
    await providers.fetchProviders();
  }

  applyFocus();
});

watch(() => [providers.focusedProvider, providers.focusedAction], applyFocus);
watch(
  () => providers.providers.length,
  () => {
    applyFocus();
  },
);

// compact, human-readable rendering of a runinator type.
function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) {
    return "any";
  }

  switch (ty.type) {
    case "array":
      return `${describeType(ty.items, depth + 1)}[]`;
    case "map":
      return `map<${describeType(ty.values, depth + 1)}>`;
    case "union":
      return ty.variants.map((variant) => describeType(variant, depth + 1)).join(" | ");
    case "enum":
      return `enum[${ty.values.map((value) => JSON.stringify(value)).join(", ")}]`;
    case "range":
      return `${describeType(ty.base, depth + 1)} range ${String(ty.min ?? "")}..${String(ty.max ?? "")}`;

    case "struct": {
      const entries = Object.entries(ty.fields);

      if (depth > 0 || entries.length > 4) {
        return "struct";
      }

      return `{ ${entries.map(([name, field]) => `${name}${field.required ? "" : "?"}: ${describeType(field.ty, depth + 1)}`).join("; ")} }`;
    }

    default:
      return ty.type;
  }
}

function shortJson(value: unknown): string {
  const text = JSON.stringify(value);
  return text.length > 48 ? `${text.slice(0, 45)}…` : text;
}
</script>

<style scoped>
.providers-pane {
  overflow: hidden;
}
.providers-layout {
  display: grid;
  height: 100%;
  min-height: 0;
  gap: 10px;
  grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
}
.providers-list-panel,
.providers-detail-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.providers-detail-panel {
  overflow: auto;
}
.providers-error {
  border-left: 3px solid var(--danger-solid);
  background: var(--danger-bg);
  color: var(--danger-fg);
  padding: 6px 8px;
  font-size: 12px;
  margin-bottom: 6px;
}
.providers-tree {
  overflow: auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.providers-group {
  display: flex;
  flex-direction: column;
}
.providers-group-head,
.providers-action {
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: var(--text);
  font: inherit;
}
.providers-group-head {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 6px 8px;
  font-weight: 600;
}
.providers-group-name {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.providers-group-count {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 600;
}
.providers-action {
  margin-left: 20px;
  padding: 4px 8px;
  font-size: 12px;
  color: var(--text-subtle);
}
.providers-group-head:hover,
.providers-action:hover {
  background: var(--surface-hover);
}
.providers-group-head.selected,
.providers-action.selected {
  border-color: var(--border-strong);
  background: var(--accent-soft);
}
.providers-detail-head h2 {
  margin: 0;
}
.providers-detail-desc {
  color: var(--text-subtle);
  font-size: 13px;
  margin: 6px 0;
}
.providers-meta-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 6px;
}
.providers-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--surface-muted);
  border-radius: 999px;
  padding: 2px 9px;
  font-size: 11px;
  color: var(--text-subtle);
}
.providers-chip.muted {
  color: var(--text-muted);
}
.providers-section-title {
  margin: 12px 0 6px;
  font-size: 13px;
}
.providers-none {
  color: var(--text-muted);
  font-size: 12px;
}
.providers-param-name {
  font-weight: 600;
  color: var(--text);
}
.providers-tag {
  display: inline-block;
  margin-left: 6px;
  border-radius: 4px;
  padding: 0 6px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
}
.providers-tag.req {
  background: var(--success-bg);
  color: var(--success-fg);
}
.providers-tag.opt {
  background: var(--surface-muted);
  color: var(--text-muted);
}
.providers-tag.secret {
  background: var(--danger-bg);
  color: var(--danger-fg);
}
.providers-dim {
  color: var(--text-faint);
}
.providers-detail-panel code {
  font:
    11px/1.4 ui-monospace,
    SFMono-Regular,
    Menlo,
    Consolas,
    monospace;
  color: var(--text);
}
.providers-action-summary {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
}
.providers-action-summary li {
  display: flex;
  gap: 10px;
  align-items: baseline;
  padding: 6px 8px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  cursor: pointer;
}
.providers-action-summary li:hover {
  background: var(--surface-hover);
}
.empty-state {
  color: var(--text-muted);
  padding: 14px 0;
}
</style>
