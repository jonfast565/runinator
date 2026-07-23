<template>
  <section class="pane h-full overflow-hidden max-md:overflow-auto">
    <div
      class="grid h-full min-h-0 gap-2.5 grid-cols-1 md:grid-cols-[minmax(220px,280px)_minmax(0,1fr)] max-md:h-auto max-md:min-h-full"
    >
      <aside class="panel flex min-h-0 flex-col max-md:max-h-[50vh]">
        <div class="panel-toolbar">
          <h2 class="m-0 text-base font-semibold text-fg">Providers</h2>
          <button class="btn" :disabled="providers.loading" @click="providers.fetchProviders()">
            <LoadingSpinner v-if="providers.loading" size="sm" label="Refreshing providers" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
        <div
          v-if="providers.error"
          class="mb-1.5 border-l-[3px] border-danger bg-danger-bg px-2 py-1.5 text-xs text-danger-fg"
        >
          {{ providers.error }}
        </div>
        <LoadingPanel v-if="providers.loading" compact message="Loading providers…" />
        <div v-else-if="!providers.providers.length" class="py-3.5 text-fg-muted">
          No providers registered.
        </div>
        <div v-else-if="!filteredProviders.length" class="py-3.5 text-fg-muted">
          No providers match "{{ app.searchQuery }}".
        </div>
        <div class="flex min-h-0 flex-col gap-0.5 overflow-auto">
          <div v-for="provider in filteredProviders" :key="provider.name" class="flex flex-col">
            <button
              type="button"
              class="flex cursor-pointer items-center gap-1.5 rounded-md border border-transparent bg-transparent px-2 py-1.5 text-left font-semibold text-fg hover:bg-surface-hover"
              :class="
                selectedProvider === provider.name && !selectedAction
                  ? 'border-border-strong bg-accent-soft'
                  : ''
              "
              @click="selectProvider(provider.name)"
            >
              <Icon name="box" :size="14" />
              <span class="min-w-0 flex-1 truncate">{{ provider.name }}</span>
              <span class="text-[11px] font-semibold text-fg-muted">{{
                provider.actions.length
              }}</span>
            </button>
            <button
              v-for="action in provider.actions"
              :key="action.function_name"
              type="button"
              class="ml-5 cursor-pointer rounded-md border border-transparent bg-transparent px-2 py-1 text-left text-xs text-fg-subtle hover:bg-surface-hover"
              :class="
                selectedProvider === provider.name && selectedAction === action.function_name
                  ? 'border-border-strong bg-accent-soft'
                  : ''
              "
              @click="selectAction(provider.name, action.function_name)"
            >
              {{ action.function_name }}
            </button>
          </div>
        </div>
      </aside>

      <section class="panel flex min-h-0 flex-col overflow-auto [&_code]:font-mono [&_code]:text-[11px] [&_code]:leading-snug [&_code]:text-fg">
        <template v-if="currentAction && currentProvider">
          <div>
            <h2 class="m-0 text-base font-semibold text-fg">
              {{ currentProvider.name }}.{{ currentAction.function_name }}
            </h2>
          </div>
          <p v-if="currentAction.description" class="my-1.5 text-[13px] text-fg-subtle">
            {{ currentAction.description }}
          </p>
          <div
            v-if="
              currentProvider.metadata.credential_scopes.length || currentProvider.metadata.contract
            "
            class="mb-1.5 flex flex-wrap gap-1.5"
          >
            <span
              v-for="scope in currentProvider.metadata.credential_scopes"
              :key="scope"
              class="inline-flex items-center gap-1 rounded-pill bg-surface-muted px-2.5 py-0.5 text-[11px] text-fg-subtle"
            >
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
            <span
              v-if="currentProvider.metadata.contract"
              class="inline-flex items-center gap-1 rounded-pill bg-surface-muted px-2.5 py-0.5 text-[11px] text-fg-muted"
              >{{ currentProvider.metadata.contract }}</span
            >
          </div>

          <h3 class="mt-3 mb-1.5 text-[13px] font-semibold text-fg">Parameters</h3>
          <div v-if="!currentAction.parameters.length" class="text-xs text-fg-muted">
            No parameters.
          </div>
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
                    <span class="font-semibold text-fg">{{ param.name }}</span>
                    <span
                      v-if="param.secret"
                      class="ml-1.5 inline-block rounded px-1.5 text-[10px] font-bold uppercase bg-danger-bg text-danger-fg"
                      >secret</span
                    >
                  </td>
                  <td>
                    <code>{{ describeType(param.ty) }}</code>
                  </td>
                  <td>
                    <span
                      class="ml-0 inline-block rounded px-1.5 text-[10px] font-bold uppercase"
                      :class="
                        param.required
                          ? 'bg-success-bg text-success-fg'
                          : 'bg-surface-muted text-fg-muted'
                      "
                      >{{ param.required ? "yes" : "no" }}</span
                    >
                  </td>
                  <td>
                    <code
                      v-if="param.default_value !== undefined && param.default_value !== null"
                      >{{ shortJson(param.default_value) }}</code
                    ><span v-else class="text-fg-faint">—</span>
                  </td>
                  <td>{{ param.description || param.label || "" }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <h3 class="mt-3 mb-1.5 text-[13px] font-semibold text-fg">Results</h3>
          <div v-if="!currentAction.results.length" class="text-xs text-fg-muted">
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
                    <span class="font-semibold text-fg">{{ result.name }}</span>
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
          <div>
            <h2 class="m-0 text-base font-semibold text-fg">{{ currentProvider.name }}</h2>
          </div>
          <div
            v-if="currentProvider.metadata.credential_scopes.length"
            class="mb-1.5 flex flex-wrap gap-1.5"
          >
            <span
              v-for="scope in currentProvider.metadata.credential_scopes"
              :key="scope"
              class="inline-flex items-center gap-1 rounded-pill bg-surface-muted px-2.5 py-0.5 text-[11px] text-fg-subtle"
            >
              <Icon name="key" :size="11" /> {{ scope }}
            </span>
          </div>
          <h3 class="mt-3 mb-1.5 text-[13px] font-semibold text-fg">Actions</h3>
          <ul class="m-0 grid list-none gap-1 p-0">
            <li
              v-for="action in currentProvider.actions"
              :key="action.function_name"
              class="flex cursor-pointer items-baseline gap-2.5 rounded-md border border-border-subtle px-2 py-1.5 hover:bg-surface-hover"
              @click="selectAction(currentProvider.name, action.function_name)"
            >
              <span class="font-semibold text-fg">{{ action.function_name }}</span>
              <span class="text-fg-faint">{{ action.description || "" }}</span>
            </li>
          </ul>
        </template>

        <div v-else class="py-3.5 text-fg-muted">Select a provider to view its actions.</div>
      </section>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import { useProvidersStore } from "../../ui/adapters/pinia/providers";
import { useAppStore } from "../../ui/adapters/pinia/app";
import type { RuninatorType } from "../../core/domain/models";

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

