<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.providers.split"
      :initial-first-pct="30"
      :min-first="260"
      :min-second="420"
      collapsible-first
      first-label="Providers"
      first-icon="box"
      mobile-mode="toggle"
      :mobile-detail-active="!!currentProvider"
    >
      <template #first>
        <aside class="panel flex min-h-0 flex-col">
          <div class="panel-toolbar">
            <div class="flex min-w-0 items-center gap-2">
              <h2 class="m-0 text-base font-semibold text-fg">Providers</h2>
              <HelpBubble
                text="Search providers by action, description, parameter, result, credential scope, or contract."
                label="About providers"
              />
              <span class="badge bg-surface-muted text-fg-muted">{{
                catalogSummary.providers
              }}</span>
            </div>
            <button
              type="button"
              class="btn"
              :disabled="providers.loading"
              @click="providers.fetchProviders()"
            >
              <LoadingSpinner v-if="providers.loading" size="sm" label="Refreshing providers" />
              <Icon v-else name="refresh" />
              <span>{{ providers.loading ? "Refreshing" : "Refresh" }}</span>
            </button>
          </div>

          <div
            v-if="providers.error && providers.providers.length"
            class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger-bg px-2.5 py-2 text-xs text-danger-fg"
            role="alert"
          >
            <Icon name="alert" :size="14" class="mt-0.5" />
            <span class="min-w-0 flex-1">Refresh failed. Showing the last loaded catalog.</span>
            <button
              type="button"
              class="shrink-0 border-0 bg-transparent p-0 font-semibold text-danger-fg underline underline-offset-2"
              @click="providers.fetchProviders()"
            >
              Retry
            </button>
          </div>

          <EmptyState
            v-if="providers.loading && !providers.providers.length"
            compact
            loading
            title="Loading providers"
          />
          <EmptyState
            v-else-if="providers.error && !providers.providers.length"
            compact
            icon="alert"
            title="Couldn't load providers"
            :description="providers.error"
          >
            <button type="button" class="btn" @click="providers.fetchProviders()">Retry</button>
          </EmptyState>
          <EmptyState
            v-else-if="!providers.providers.length"
            compact
            icon="box"
            title="No providers registered"
            description="Start a provider worker or desktop agent, then refresh this catalog."
          />
          <EmptyState
            v-else-if="!catalogMatches.length"
            compact
            icon="search"
            title="No matching contracts"
            :description="`Nothing in the provider catalog matches ${app.searchQuery}.`"
          >
            <button type="button" class="btn" @click="app.searchQuery = ''">Clear search</button>
          </EmptyState>

          <template v-else>
            <div class="flex items-center justify-between gap-2 text-[11px] text-fg-muted">
              <span v-if="app.normalizedSearch">
                {{ catalogMatches.length }} {{ pluralize(catalogMatches.length, "provider") }} ·
                {{ visibleActionCount }} {{ pluralize(visibleActionCount, "action") }}
              </span>
              <span v-else>
                {{ catalogSummary.actions }} {{ pluralize(catalogSummary.actions, "action") }} ·
                {{ catalogSummary.credentialScopes }} credential
                {{ pluralize(catalogSummary.credentialScopes, "scope") }}
              </span>
              <span v-if="app.normalizedSearch" class="truncate" :title="app.searchQuery">
                for “{{ app.searchQuery }}”
              </span>
            </div>

            <div class="-mx-1 flex min-h-0 flex-1 flex-col gap-1 overflow-auto px-1 pb-1">
              <div v-for="match in catalogMatches" :key="match.provider.name" class="flex flex-col">
                <button
                  type="button"
                  class="group flex cursor-pointer items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors"
                  :class="
                    selectedProvider === match.provider.name
                      ? 'border-accent bg-accent-soft text-accent-text'
                      : 'border-transparent bg-transparent text-fg hover:border-border-subtle hover:bg-surface-hover'
                  "
                  :aria-expanded="providerActionsVisible(match.provider.name)"
                  :aria-current="selectedProvider === match.provider.name ? 'true' : undefined"
                  @click="selectProvider(match.provider.name)"
                >
                  <span
                    class="inline-flex size-7 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-surface"
                  >
                    <Icon name="box" :size="14" />
                  </span>
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[13px] font-semibold">{{
                      match.provider.name
                    }}</span>
                    <span class="block truncate text-[11px] font-normal text-fg-muted">
                      {{ providerSubtitle(match.provider) }}
                    </span>
                  </span>
                  <span class="text-[11px] font-semibold text-fg-muted">
                    {{ match.actions.length
                    }}<template v-if="match.actions.length !== match.provider.actions.length"
                      >/{{ match.provider.actions.length }}</template
                    >
                  </span>
                  <Icon
                    :name="
                      providerActionsVisible(match.provider.name) ? 'arrow-down' : 'chevron-right'
                    "
                    :size="13"
                  />
                </button>

                <div
                  v-if="providerActionsVisible(match.provider.name)"
                  class="ml-5 border-l border-border-subtle py-1 pl-2"
                >
                  <button
                    v-for="action in match.actions"
                    :key="action.function_name"
                    type="button"
                    class="mb-0.5 flex w-full cursor-pointer items-center gap-2 rounded-md border px-2 py-1.5 text-left text-xs transition-colors last:mb-0"
                    :class="
                      selectedProvider === match.provider.name &&
                      selectedAction === action.function_name
                        ? 'border-border-strong bg-surface text-fg shadow-control'
                        : 'border-transparent bg-transparent text-fg-subtle hover:bg-surface-hover'
                    "
                    :aria-current="
                      selectedProvider === match.provider.name &&
                      selectedAction === action.function_name
                        ? 'true'
                        : undefined
                    "
                    @click="selectAction(match.provider.name, action.function_name)"
                  >
                    <span class="min-w-0 flex-1 truncate font-mono">{{
                      action.function_name
                    }}</span>
                    <span class="shrink-0 text-[10px] text-fg-faint">
                      {{ action.parameters.length }}→{{ action.results.length }}
                    </span>
                  </button>
                  <div v-if="!match.actions.length" class="px-2 py-1.5 text-xs text-fg-muted">
                    No actions declared.
                  </div>
                </div>
              </div>
            </div>
          </template>
        </aside>
      </template>

      <template #second>
        <section
          class="panel details overflow-auto [&_code]:font-mono [&_code]:text-[11px] [&_code]:leading-snug [&_code]:text-fg"
        >
          <MobileBackBar label="Back to providers" @back="clearSelection" />

          <template v-if="currentAction && currentProvider">
            <div class="flex flex-wrap items-center gap-1 text-xs text-fg-muted">
              <button
                type="button"
                class="cursor-pointer border-0 bg-transparent p-0 font-semibold text-accent-text hover:underline"
                @click="selectProvider(currentProvider.name)"
              >
                {{ currentProvider.name }}
              </button>
              <Icon name="chevron-right" :size="12" />
              <span class="font-mono text-fg-subtle">{{ currentAction.function_name }}</span>
            </div>

            <div class="detail-header">
              <div class="min-w-0">
                <h2 class="m-0 break-words text-lg font-semibold text-fg">
                  {{ currentAction.function_name }}
                </h2>
                <p
                  v-if="currentAction.description"
                  class="mt-1 mb-0 max-w-4xl text-[13px] text-fg-subtle"
                >
                  {{ currentAction.description }}
                </p>
              </div>
              <button type="button" class="btn" @click="copyReference">
                <Icon :name="copiedReference ? 'check' : 'file'" />
                <span>{{ copiedReference ? "Copied" : "Copy reference" }}</span>
              </button>
            </div>

            <div class="flex flex-wrap gap-1.5" aria-label="Action metadata">
              <span
                class="badge"
                :class="
                  currentAction.pure ? 'bg-info-bg text-info-fg' : 'bg-warning-bg text-warning-fg'
                "
              >
                <Icon :name="currentAction.pure ? 'bolt' : 'gear'" :size="11" />
                {{ currentAction.pure ? "Pure" : "Effect" }}
              </span>
              <span v-if="!currentAction.pure" class="badge bg-surface-muted text-fg-subtle">
                {{ deliveryLabel(currentAction.delivery_semantics) }}
              </span>
              <span
                v-if="currentProvider.metadata.contract"
                class="badge bg-surface-muted text-fg-subtle"
              >
                {{ currentProvider.metadata.contract }} contract
              </span>
              <span
                v-for="scope in currentProvider.metadata.credential_scopes"
                :key="scope"
                class="badge bg-surface-muted text-fg-subtle"
              >
                <Icon name="key" :size="11" />{{ scope }}
              </span>
            </div>

            <div class="grid grid-cols-[repeat(auto-fit,minmax(130px,1fr))] gap-2">
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Inputs</span
                >
                <strong class="text-lg text-fg">{{ currentAction.parameters.length }}</strong>
              </div>
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Required</span
                >
                <strong class="text-lg text-fg">{{ requiredParameterCount }}</strong>
              </div>
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Outputs</span
                >
                <strong class="text-lg text-fg">{{ currentAction.results.length }}</strong>
              </div>
            </div>

            <section class="grid gap-2" aria-labelledby="provider-action-inputs">
              <div class="flex items-center justify-between gap-2">
                <h3 id="provider-action-inputs" class="text-[13px] font-semibold text-fg">
                  Inputs
                </h3>
                <span class="text-xs text-fg-muted">{{ currentAction.parameters.length }}</span>
              </div>
              <div
                v-if="!currentAction.parameters.length"
                class="rounded-md border border-dashed border-border px-3 py-3 text-xs text-fg-muted"
              >
                This action takes no inputs.
              </div>
              <ul v-else class="m-0 grid list-none gap-2 p-0">
                <li
                  v-for="param in currentAction.parameters"
                  :key="param.name"
                  class="rounded-md border border-border-subtle bg-surface-subtle p-3"
                >
                  <div class="flex flex-wrap items-center gap-2">
                    <code class="mr-auto font-semibold">{{ param.name }}</code>
                    <span
                      class="badge"
                      :class="
                        param.required
                          ? 'bg-success-bg text-success-fg'
                          : 'bg-surface-muted text-fg-muted'
                      "
                    >
                      {{ param.required ? "Required" : "Optional" }}
                    </span>
                    <span v-if="param.secret" class="badge bg-danger-bg text-danger-fg">
                      <Icon name="lock" :size="11" />Secret
                    </span>
                  </div>
                  <p
                    v-if="param.description || param.label"
                    class="mt-2 mb-0 text-xs leading-relaxed text-fg-subtle"
                  >
                    {{ param.description || param.label }}
                  </p>
                  <div
                    class="mt-2 grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-2 text-xs"
                  >
                    <div class="min-w-0">
                      <span
                        class="block text-[10px] font-semibold uppercase tracking-wide text-fg-muted"
                        >Type</span
                      >
                      <code class="mt-0.5 block break-words whitespace-normal">{{
                        describeType(param.ty)
                      }}</code>
                    </div>
                    <div
                      v-if="param.default_value !== undefined && param.default_value !== null"
                      class="min-w-0"
                    >
                      <span
                        class="block text-[10px] font-semibold uppercase tracking-wide text-fg-muted"
                        >Default</span
                      >
                      <code class="mt-0.5 block break-words whitespace-normal">{{
                        shortJson(param.default_value)
                      }}</code>
                    </div>
                  </div>
                </li>
              </ul>
            </section>

            <section class="grid gap-2" aria-labelledby="provider-action-outputs">
              <div class="flex items-center justify-between gap-2">
                <h3 id="provider-action-outputs" class="text-[13px] font-semibold text-fg">
                  Outputs
                </h3>
                <span class="text-xs text-fg-muted">{{ currentAction.results.length }}</span>
              </div>
              <div
                v-if="!currentAction.results.length"
                class="rounded-md border border-dashed border-border px-3 py-3 text-xs text-fg-muted"
              >
                This action declares no outputs.
              </div>
              <ul v-else class="m-0 grid list-none gap-2 p-0">
                <li
                  v-for="result in currentAction.results"
                  :key="result.name"
                  class="rounded-md border border-border-subtle bg-surface-subtle p-3"
                >
                  <code class="font-semibold">{{ result.name }}</code>
                  <p
                    v-if="result.description || result.label"
                    class="mt-2 mb-0 text-xs leading-relaxed text-fg-subtle"
                  >
                    {{ result.description || result.label }}
                  </p>
                  <div class="mt-2 text-xs">
                    <span
                      class="block text-[10px] font-semibold uppercase tracking-wide text-fg-muted"
                      >Type</span
                    >
                    <code class="mt-0.5 block break-words whitespace-normal">{{
                      describeType(result.ty)
                    }}</code>
                  </div>
                </li>
              </ul>
            </section>
          </template>

          <template v-else-if="currentProvider">
            <div class="detail-header">
              <div class="flex min-w-0 items-center gap-3">
                <span
                  class="inline-flex size-10 shrink-0 items-center justify-center rounded-md border border-accent/25 bg-accent-soft text-accent-text"
                >
                  <Icon name="box" :size="20" />
                </span>
                <div class="min-w-0">
                  <h2 class="m-0 truncate text-lg font-semibold text-fg">
                    {{ currentProvider.name }}
                  </h2>
                  <p class="mt-0.5 mb-0 text-xs text-fg-muted">Registered provider contract</p>
                </div>
              </div>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <span
                v-if="currentProvider.metadata.contract"
                class="badge bg-surface-muted text-fg-subtle"
              >
                {{ currentProvider.metadata.contract }} contract
              </span>
              <span
                v-for="scope in currentProvider.metadata.credential_scopes"
                :key="scope"
                class="badge bg-surface-muted text-fg-subtle"
              >
                <Icon name="key" :size="11" />{{ scope }}
              </span>
              <span
                v-if="!currentProvider.metadata.credential_scopes.length"
                class="text-xs text-fg-muted"
              >
                No credential scopes required.
              </span>
            </div>

            <div class="grid grid-cols-[repeat(auto-fit,minmax(130px,1fr))] gap-2">
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Actions</span
                >
                <strong class="text-lg text-fg">{{ currentProviderSummary.actions }}</strong>
              </div>
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Inputs</span
                >
                <strong class="text-lg text-fg">{{ currentProviderSummary.parameters }}</strong>
              </div>
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Outputs</span
                >
                <strong class="text-lg text-fg">{{ currentProviderSummary.results }}</strong>
              </div>
              <div class="metric-card">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
                  >Scopes</span
                >
                <strong class="text-lg text-fg">{{
                  currentProviderSummary.credentialScopes
                }}</strong>
              </div>
            </div>

            <section class="grid gap-2" aria-labelledby="provider-actions">
              <div class="flex items-center justify-between gap-2">
                <h3 id="provider-actions" class="text-[13px] font-semibold text-fg">Actions</h3>
                <span class="text-xs text-fg-muted">{{ currentProvider.actions.length }}</span>
              </div>
              <div
                v-if="!currentProvider.actions.length"
                class="rounded-md border border-dashed border-border px-3 py-3 text-xs text-fg-muted"
              >
                This provider declares no actions.
              </div>
              <div v-else class="grid grid-cols-[repeat(auto-fit,minmax(260px,1fr))] gap-2">
                <button
                  v-for="action in currentProvider.actions"
                  :key="action.function_name"
                  type="button"
                  class="group grid cursor-pointer gap-2 rounded-md border border-border-subtle bg-surface-subtle p-3 text-left transition-all hover:-translate-y-px hover:border-accent hover:bg-surface hover:shadow-control"
                  @click="selectAction(currentProvider.name, action.function_name)"
                >
                  <span class="flex items-center gap-2">
                    <code class="min-w-0 flex-1 truncate font-semibold">{{
                      action.function_name
                    }}</code>
                    <Icon
                      name="chevron-right"
                      :size="14"
                      class="text-fg-muted group-hover:text-accent-text"
                    />
                  </span>
                  <span class="min-h-8 text-xs leading-relaxed text-fg-subtle">
                    {{ action.description || "No description provided." }}
                  </span>
                  <span class="flex flex-wrap gap-1.5 text-[10px] text-fg-muted">
                    <span
                      >{{ action.parameters.length }}
                      {{ pluralize(action.parameters.length, "input") }}</span
                    >
                    <span aria-hidden="true">·</span>
                    <span
                      >{{ action.results.length }}
                      {{ pluralize(action.results.length, "output") }}</span
                    >
                    <span aria-hidden="true">·</span>
                    <span>{{ action.pure ? "Pure" : "Effect" }}</span>
                  </span>
                </button>
              </div>
            </section>
          </template>

          <EmptyState
            v-else
            class="min-h-full"
            icon="box"
            :title="providers.providers.length ? 'Select a provider' : 'Provider details'"
            :description="
              providers.providers.length
                ? 'Choose a provider or action to inspect its callable contract.'
                : 'Registered provider contracts will appear here.'
            "
          />
        </section>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DeliverySemantics, ProviderMetadata, RuninatorType } from "../../core/domain/models";
import { searchProviderCatalog, summarizeProviderCatalog } from "../../core/utils/provider-catalog";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useProvidersStore } from "../../ui/adapters/pinia/providers";
import EmptyState from "../components/shared/EmptyState.vue";
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";

const providers = useProvidersStore();
const app = useAppStore();
const selectedProvider = ref("");
const selectedAction = ref("");
const copiedReference = ref(false);
let copiedTimer: ReturnType<typeof window.setTimeout> | null = null;

const catalogMatches = computed(() =>
  searchProviderCatalog(providers.providers, app.normalizedSearch),
);
const catalogSummary = computed(() => summarizeProviderCatalog(providers.providers));
const visibleActionCount = computed(() =>
  catalogMatches.value.reduce((count, match) => count + match.actions.length, 0),
);
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
const currentProviderSummary = computed(() =>
  summarizeProviderCatalog(currentProvider.value ? [currentProvider.value] : []),
);
const requiredParameterCount = computed(
  () => currentAction.value?.parameters.filter((parameter) => parameter.required).length ?? 0,
);

function selectProvider(name: string) {
  selectedProvider.value = name;
  selectedAction.value = "";
}

function selectAction(provider: string, action: string) {
  selectedProvider.value = provider;
  selectedAction.value = action;
}

function clearSelection() {
  selectedProvider.value = "";
  selectedAction.value = "";
}

function providerActionsVisible(name: string): boolean {
  return Boolean(app.normalizedSearch) || selectedProvider.value === name;
}

function providerSubtitle(provider: ProviderMetadata): string {
  if (provider.metadata.contract) {
    return `${provider.metadata.contract} contract`;
  }

  if (provider.metadata.credential_scopes.length) {
    const scopeCount = provider.metadata.credential_scopes.length;
    return `${String(scopeCount)} credential ${pluralize(scopeCount, "scope")}`;
  }

  return "No credentials required";
}

// Apply a deep-linked focus, then keep selection valid as workers refresh the provider catalog.
function applyFocus() {
  if (providers.focusedProvider) {
    const provider = providers.providers.find(
      (candidate) => candidate.name === providers.focusedProvider,
    );
    const action = provider?.actions.find(
      (candidate) => candidate.function_name === providers.focusedAction,
    );

    if (provider) {
      selectedProvider.value = provider.name;
      selectedAction.value = action?.function_name ?? "";
    }

    providers.focusProviderAction("", "");
  }

  const provider = providers.providers.find(
    (candidate) => candidate.name === selectedProvider.value,
  );

  if (!provider) {
    selectedProvider.value = providers.providers[0]?.name ?? "";
    selectedAction.value = "";
    return;
  }

  if (
    selectedAction.value &&
    !provider.actions.some((action) => action.function_name === selectedAction.value)
  ) {
    selectedAction.value = "";
  }
}

async function copyReference() {
  if (!currentProvider.value || !currentAction.value) {
    return;
  }

  try {
    await navigator.clipboard.writeText(
      `${currentProvider.value.name}.${currentAction.value.function_name}`,
    );
    copiedReference.value = true;

    if (copiedTimer !== null) {
      window.clearTimeout(copiedTimer);
    }

    copiedTimer = window.setTimeout(() => {
      copiedReference.value = false;
      copiedTimer = null;
    }, 1800);
  } catch {
    // Clipboard access may be unavailable in an embedded webview; selection remains usable.
  }
}

onMounted(async () => {
  if (!providers.providers.length) {
    await providers.fetchProviders();
  }

  applyFocus();
});

onBeforeUnmount(() => {
  if (copiedTimer !== null) {
    window.clearTimeout(copiedTimer);
  }
});

watch(() => [providers.focusedProvider, providers.focusedAction], applyFocus);
watch(() => providers.providers.map((provider) => provider.name).join("\u0000"), applyFocus);
watch(catalogMatches, (matches) => {
  if (
    app.normalizedSearch &&
    matches.length &&
    !matches.some((match) => match.provider.name === selectedProvider.value)
  ) {
    selectProvider(matches[0].provider.name);
  }
});

function deliveryLabel(semantics: DeliverySemantics | undefined): string {
  switch (semantics) {
    case "idempotent":
      return "Idempotent delivery";
    case "reconcilable":
      return "Reconcilable delivery";
    default:
      return "At-least-once delivery";
  }
}

function pluralize(count: number, noun: string): string {
  return count === 1 ? noun : `${noun}s`;
}

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
  return text.length > 72 ? `${text.slice(0, 69)}…` : text;
}
</script>
