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
        <div class="providers-tree">
          <div v-for="provider in providers.providers" :key="provider.name" class="providers-group">
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
              :class="{ selected: selectedProvider === provider.name && selectedAction === action.function_name }"
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
          <p v-if="currentAction.description" class="providers-detail-desc">{{ currentAction.description }}</p>
          <div v-if="currentProvider.metadata.credential_scopes.length || currentProvider.metadata.contract" class="providers-meta-row">
            <span v-for="scope in currentProvider.metadata.credential_scopes" :key="scope" class="providers-chip">
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
            <span v-if="currentProvider.metadata.contract" class="providers-chip muted">{{ currentProvider.metadata.contract }}</span>
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
                  <td><code>{{ describeType(param.ty) }}</code></td>
                  <td>
                    <span :class="['providers-tag', param.required ? 'req' : 'opt']">{{ param.required ? "yes" : "no" }}</span>
                  </td>
                  <td><code v-if="param.default_value !== undefined && param.default_value !== null">{{ shortJson(param.default_value) }}</code><span v-else class="providers-dim">—</span></td>
                  <td>{{ param.description || param.label || "" }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <h3 class="providers-section-title">Results</h3>
          <div v-if="!currentAction.results.length" class="providers-none">No declared results.</div>
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
                  <td><span class="providers-param-name">{{ result.name }}</span></td>
                  <td><code>{{ describeType(result.ty) }}</code></td>
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
            <span v-for="scope in currentProvider.metadata.credential_scopes" :key="scope" class="providers-chip">
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
          </div>
          <h3 class="providers-section-title">Actions</h3>
          <ul class="providers-action-summary">
            <li v-for="action in currentProvider.actions" :key="action.function_name" @click="selectAction(currentProvider.name, action.function_name)">
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
import type { RuninatorType } from "../types/models";

const providers = useProvidersStore();
const selectedProvider = ref("");
const selectedAction = ref("");

const currentProvider = computed(() => providers.providers.find((provider) => provider.name === selectedProvider.value) ?? null);
const currentAction = computed(() => {
  if (!selectedAction.value) return null;
  return currentProvider.value?.actions.find((action) => action.function_name === selectedAction.value) ?? null;
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
  if (!providers.providers.length) await providers.fetchProviders();
  applyFocus();
});

watch(() => [providers.focusedProvider, providers.focusedAction], applyFocus);
watch(() => providers.providers.length, () => applyFocus());

// compact, human-readable rendering of a runinator type.
function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) return "any";
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
      return `${describeType(ty.base, depth + 1)} range ${ty.min ?? ""}..${ty.max ?? ""}`;
    case "struct": {
      const entries = Object.entries(ty.fields);
      if (depth > 0 || entries.length > 4) return "struct";
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
  border-left: 3px solid #dc2626;
  background: #fff1f1;
  color: #9f1239;
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
  color: #17202b;
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
  color: #66717e;
  font-size: 11px;
  font-weight: 600;
}
.providers-action {
  margin-left: 20px;
  padding: 4px 8px;
  font-size: 12px;
  color: #344255;
}
.providers-group-head:hover,
.providers-action:hover {
  background: #f1f5fb;
}
.providers-group-head.selected,
.providers-action.selected {
  border-color: #b7c8dc;
  background: #eef5ff;
}
.providers-detail-head h2 {
  margin: 0;
}
.providers-detail-desc {
  color: #4b5663;
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
  background: #eef2f7;
  border-radius: 999px;
  padding: 2px 9px;
  font-size: 11px;
  color: #344255;
}
.providers-chip.muted {
  color: #66717e;
}
.providers-section-title {
  margin: 12px 0 6px;
  font-size: 13px;
}
.providers-none {
  color: #66717e;
  font-size: 12px;
}
.providers-param-name {
  font-weight: 600;
  color: #17202b;
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
  background: #dcfce7;
  color: #166534;
}
.providers-tag.opt {
  background: #eef2f7;
  color: #66717e;
}
.providers-tag.secret {
  background: #fde8e8;
  color: #b91c1c;
}
.providers-dim {
  color: #97a1ad;
}
.providers-detail-panel code {
  font: 11px/1.4 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: #1c2938;
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
  border: 1px solid #e3e8ee;
  border-radius: 6px;
  cursor: pointer;
}
.providers-action-summary li:hover {
  background: #f1f5fb;
}
.empty-state {
  color: #66717e;
  padding: 14px 0;
}
</style>
