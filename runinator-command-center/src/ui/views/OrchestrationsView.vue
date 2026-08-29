<template>
  <section class="pane flex h-full min-h-0 flex-col gap-2.5 overflow-hidden">
    <div class="panel shrink-0">
      <div class="panel-toolbar">
        <div>
          <h2 class="m-0 text-base font-semibold text-fg">Orchestrations</h2>
          <p class="mt-1 mb-0 text-sm text-fg-muted">
            Generic correlations, immutable execution epochs, adapters, and provider effects.
          </p>
        </div>
        <div class="btn-row">
          <button
            v-for="item in modes"
            :key="item"
            type="button"
            class="btn"
            :class="mode === item ? 'btn-primary' : ''"
            @click="switchMode(item)"
          >
            {{ item }}
          </button>
        </div>
      </div>

      <p
        v-if="store.error"
        class="rounded-md border border-danger bg-danger-bg p-3 text-sm text-danger-fg"
      >
        {{ store.error }}
      </p>

      <div v-if="mode === 'Instances'" class="flex flex-wrap items-end gap-2">
        <label class="grid gap-1 text-xs text-fg-muted"
          ><span>Status</span
          ><select v-model="filters.status" class="w-auto min-w-40" @change="refreshInstances">
            <option value="">All statuses</option>
            <option v-for="item in statuses" :key="item" :value="item">{{ item }}</option>
          </select></label
        >
        <label class="grid gap-1 text-xs text-fg-muted"
          ><span>Scope</span
          ><input
            v-model="filters.scope"
            class="w-auto min-w-40"
            placeholder="Any scope"
            @keyup.enter="refreshInstances"
        /></label>
        <label class="grid gap-1 text-xs text-fg-muted"
          ><span>Correlation key</span
          ><input
            v-model="filters.correlation_key"
            class="w-auto min-w-56"
            placeholder="octo/repo#42"
            @keyup.enter="refreshInstances"
        /></label>
        <label class="grid gap-1 text-xs text-fg-muted"
          ><span>Pipeline ID</span
          ><input
            v-model="filters.pipeline_id"
            class="w-auto min-w-56"
            @keyup.enter="refreshInstances"
        /></label>
        <label class="grid gap-1 text-xs text-fg-muted"
          ><span>Adapter ID</span
          ><input
            v-model="filters.adapter_id"
            class="w-auto min-w-56"
            @keyup.enter="refreshInstances"
        /></label>
        <button class="btn" :disabled="store.loading" @click="refreshInstances">
          <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing orchestrations" />
          <Icon v-else name="refresh" />
          <span>Refresh</span>
        </button>
      </div>

      <div v-else class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p class="m-0 text-sm font-medium text-fg">Provider adapters</p>
          <p class="mt-1 mb-0 text-xs text-fg-muted">
            Receive provider events, normalize them, and route them into orchestrations.
          </p>
        </div>
        <div class="btn-row flex-wrap">
          <button class="btn btn-primary" @click="openAdapterForm()">
            <Icon name="plus" />
            <span>New adapter</span>
          </button>
          <button class="btn" :disabled="store.loading" @click="refreshAdapters">
            <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing adapters" />
            <Icon v-else name="refresh" />
            <span>Refresh</span>
          </button>
        </div>
      </div>
    </div>

    <template v-if="mode === 'Instances'">
      <SplitPane
        class="min-h-0 flex-1"
        storage-key="command-center.orchestrations.instances.split"
        :initial-first-pct="28"
        :min-first="260"
        :min-second="420"
        collapsible-first
        first-label="Orchestrations"
        first-icon="branch"
        mobile-mode="toggle"
        :mobile-detail-active="!!store.selected && !showInstanceList"
      >
        <template #first>
          <aside class="panel overflow-auto p-0">
            <button
              v-for="binding in store.bindings"
              :key="binding.id"
              class="block w-full border-b border-border p-3 text-left hover:bg-surface-hover"
              :class="{ 'bg-surface-muted': binding.id === store.selectedId }"
              @click="openBinding(binding.id)"
            >
              <div class="flex items-center justify-between gap-2">
                <span class="truncate font-medium text-fg">{{ binding.correlation_key }}</span
                ><span class="rounded bg-surface-subtle px-2 py-0.5 text-xs text-fg-muted">{{
                  binding.status
                }}</span>
              </div>
              <div class="mt-1 truncate text-xs text-fg-muted">
                {{ binding.scope }} · generation {{ binding.generation }} · epoch
                {{ binding.current_epoch }}
              </div>
            </button>
            <EmptyState
              v-if="!store.loading && store.bindings.length === 0"
              compact
              icon="search"
              title="No orchestrations match these filters"
            />
          </aside>
        </template>

        <template #second>
          <div class="panel details overflow-auto">
            <MobileBackBar label="Back to orchestrations" @back="showInstanceList = true" />
            <main v-if="store.selected">
              <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h2 class="text-lg font-semibold text-fg">
                    {{ store.selected.correlation_key }}
                  </h2>
                  <div class="mt-2 flex flex-wrap gap-2 text-xs text-fg-muted">
                    <span
                      v-for="chip in instanceChips"
                      :key="chip"
                      class="rounded bg-surface-subtle px-2 py-1"
                      >{{ chip }}</span
                    >
                  </div>
                </div>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="(_, name) in store.selected.policy.intents"
                    :key="name"
                    class="btn"
                    @click="openIntent(String(name))"
                  >
                    {{ name }}</button
                  ><button v-if="isSelectedTerminal" class="btn" @click="openRequeue">
                    Requeue generation
                  </button>
                </div>
              </div>
              <nav class="mt-5 flex flex-wrap gap-1 border-b border-border">
                <button
                  v-for="tab in instanceTabs"
                  :key="tab"
                  class="px-3 py-2 text-sm"
                  :class="
                    tab === activeInstanceTab ? 'border-b-2 border-accent text-fg' : 'text-fg-muted'
                  "
                  @click="activeInstanceTab = tab"
                >
                  {{ tab }}
                </button>
              </nav>
              <div class="mt-4">
                <div v-if="activeInstanceTab === 'Timeline'" class="space-y-2">
                  <article
                    v-for="event in store.events"
                    :key="event.id"
                    class="rounded border border-border p-3 text-sm"
                  >
                    <div class="flex justify-between gap-3">
                      <strong>#{{ event.sequence }} {{ event.winner || "observed" }}</strong
                      ><span class="text-fg-muted">{{ event.disposition }}</span>
                    </div>
                    <p class="mt-1 text-xs text-fg-muted">
                      matched: {{ event.matched_intents.join(", ") || "none" }} · suppressed:
                      {{ event.suppressed_intents.join(", ") || "none" }}
                    </p>
                    <pre class="mt-2 overflow-auto text-xs">{{ pretty(event.detail) }}</pre>
                  </article>
                </div>
                <div v-else-if="activeInstanceTab === 'Epochs'" class="space-y-2">
                  <section
                    v-if="currentEpochRunId"
                    class="grid gap-2 rounded border border-border p-3"
                  >
                    <div class="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <strong>Current epoch execution graph</strong>
                        <p class="text-xs text-fg-muted">
                          Immutable pipeline snapshot for epoch {{ store.selected.current_epoch }}
                        </p>
                      </div>
                      <button class="btn" @click="openPipelineRun(currentEpochRunId)">
                        Open pipeline run
                      </button>
                    </div>
                    <div
                      v-if="currentEpochDetail"
                      class="h-[360px] min-h-[260px] overflow-hidden rounded border border-border bg-surface-subtle"
                    >
                      <PipelineCanvas
                        :detail="currentEpochDetail"
                        readonly
                        @open-run="openWorkflowRun"
                      />
                    </div>
                    <p v-else class="text-sm text-fg-muted">
                      {{
                        pipelineRuns.detailLoading
                          ? "Loading execution graph…"
                          : "Execution graph unavailable."
                      }}
                    </p>
                  </section>
                  <article
                    v-for="epoch in store.epochs"
                    :key="epoch.id"
                    class="rounded border border-border p-3 text-sm"
                  >
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>Epoch {{ epoch.epoch }}</strong>
                        <p class="text-xs text-fg-muted">
                          starts at {{ epoch.start_member || "pipeline entry" }} ·
                          {{ epoch.reason }}
                        </p>
                      </div>
                      <span class="rounded bg-surface-subtle px-2 py-1 text-xs">{{
                        epoch.status
                      }}</span>
                    </div>
                    <button
                      v-if="epoch.pipeline_run_id"
                      class="btn mt-3"
                      @click="openPipelineRun(epoch.pipeline_run_id)"
                    >
                      Open pipeline run
                    </button>
                    <details class="mt-2">
                      <summary class="cursor-pointer text-xs text-fg-muted">
                        Epoch parameters
                      </summary>
                      <pre class="mt-2 overflow-auto text-xs">{{ pretty(epoch.parameters) }}</pre>
                    </details>
                  </article>
                  <p v-if="store.epochs.length === 0" class="text-sm text-fg-muted">
                    No execution epoch has been created.
                  </p>
                </div>
                <div v-else-if="activeInstanceTab === 'Evidence'" class="space-y-2">
                  <div v-if="store.evidence.length" class="flex justify-end">
                    <button class="btn" @click="downloadAllEvidence">Download all evidence</button>
                  </div>
                  <article
                    v-for="item in store.evidence"
                    :key="item.id"
                    class="rounded border border-border p-3 text-sm"
                  >
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>{{ item.kind }}</strong>
                        <p class="text-xs text-fg-muted">
                          epoch {{ item.epoch ?? "—" }} · revision
                          {{ item.subject_revision ?? "—" }}
                        </p>
                      </div>
                      <button class="btn" @click="downloadEvidence(item)">Download JSON</button>
                    </div>
                    <pre class="mt-2 max-h-80 overflow-auto text-xs">{{
                      pretty(item.payload)
                    }}</pre>
                  </article>
                  <p v-if="store.evidence.length === 0" class="text-sm text-fg-muted">
                    No evidence has been recorded.
                  </p>
                </div>
                <pre v-else-if="activeInstanceTab === 'Resources'" class="overflow-auto text-xs">{{
                  pretty(store.selected.resources)
                }}</pre>
                <pre v-else-if="activeInstanceTab === 'Budgets'" class="overflow-auto text-xs">{{
                  pretty({
                    consumed: store.selected.budgets,
                    policy: store.selected.policy.budgets,
                  })
                }}</pre>
                <div v-else-if="activeInstanceTab === 'Operations'" class="space-y-2">
                  <article
                    v-for="operation in store.operations"
                    :key="operation.id"
                    class="rounded border border-border p-3 text-sm"
                  >
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>{{ operation.provider }}.{{ operation.action }}</strong>
                        <p class="text-xs text-fg-muted">
                          {{ operation.semantics }} · attempt {{ operation.attempt }} · epoch
                          {{ operation.epoch }}
                        </p>
                      </div>
                      <span class="rounded bg-surface-subtle px-2 py-1 text-xs"
                        >{{ operation.status
                        }}<template v-if="operation.ambiguous"> · ambiguous</template></span
                      >
                    </div>
                    <code class="mt-2 block break-all text-xs text-fg-muted">{{
                      operation.operation_key
                    }}</code>
                    <div
                      v-if="operation.status === 'waiting' || operation.ambiguous"
                      class="mt-3 flex gap-2"
                    >
                      <button class="btn" @click="openResolution(operation, 'succeeded')">
                        Mark succeeded</button
                      ><button class="btn" @click="openResolution(operation, 'failed')">
                        Mark failed</button
                      ><button
                        v-if="operation.semantics !== 'at_least_once'"
                        class="btn"
                        @click="openResolution(operation, 'retry')"
                      >
                        Retry safely
                      </button>
                    </div>
                  </article>
                  <p v-if="store.operations.length === 0" class="text-sm text-fg-muted">
                    No binding-scoped provider operations.
                  </p>
                </div>
                <div v-else-if="activeInstanceTab === 'Workspaces'" class="space-y-2">
                  <article
                    v-for="workspace in store.workspaces"
                    :key="workspace.id"
                    class="rounded border border-border p-3 text-sm"
                  >
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>{{ workspace.scope }}</strong>
                        <p class="text-xs text-fg-muted">
                          attempt {{ workspace.attempt }} · {{ workspace.worker_instance_id }}
                        </p>
                      </div>
                      <span class="rounded bg-surface-subtle px-2 py-1 text-xs"
                        >{{ workspace.status }} · CAS {{ workspace.version }}</span
                      >
                    </div>
                    <code class="mt-2 block break-all text-xs text-fg-muted">{{
                      workspace.local_key
                    }}</code>
                    <p class="mt-2 text-xs text-fg-muted">
                      lease until {{ workspace.leased_until }}
                    </p>
                    <details v-if="workspace.evidence !== null" class="mt-2">
                      <summary class="cursor-pointer text-xs">Evidence</summary>
                      <pre class="mt-2 overflow-auto text-xs">{{ pretty(workspace.evidence) }}</pre>
                      <button class="btn mt-2" @click="downloadWorkspaceEvidence(workspace)">
                        Download evidence JSON
                      </button>
                    </details>
                  </article>
                  <p v-if="store.workspaces.length === 0" class="text-sm text-fg-muted">
                    No workspaces allocated for this generation.
                  </p>
                </div>
                <div v-else-if="activeInstanceTab === 'Aliases'" class="space-y-3">
                  <form
                    class="grid gap-2 rounded border border-border p-3 md:grid-cols-[1fr_1fr_1fr_auto]"
                    @submit.prevent="submitAlias"
                  >
                    <label class="grid gap-1 text-xs"
                      ><span>Source</span
                      ><input v-model="aliasSource" required placeholder="github"
                    /></label>
                    <label class="grid gap-1 text-xs"
                      ><span>Scope</span
                      ><input v-model="aliasScope" required placeholder="pull-requests"
                    /></label>
                    <label class="grid gap-1 text-xs"
                      ><span>Correlation key</span
                      ><input v-model="aliasCorrelation" required placeholder="octo/repo#42"
                    /></label>
                    <button class="btn self-end" type="submit">Add alias</button>
                  </form>
                  <article
                    v-for="alias in store.aliases"
                    :key="alias.id"
                    class="flex flex-wrap items-start justify-between gap-3 rounded border border-border p-3 text-sm"
                  >
                    <div>
                      <strong>{{ alias.correlation_key }}</strong>
                      <p class="text-xs text-fg-muted">
                        {{ alias.source }} · {{ alias.scope }} · generation {{ alias.generation }}
                      </p>
                    </div>
                    <button class="btn" @click="store.removeAlias(alias.id)">Remove</button>
                  </article>
                  <p v-if="store.aliases.length === 0" class="text-sm text-fg-muted">
                    No alternate correlation identities route to this generation.
                  </p>
                </div>
                <pre v-else-if="activeInstanceTab === 'Commands'" class="overflow-auto text-xs">{{
                  pretty(store.commands)
                }}</pre>
                <pre v-else class="overflow-auto text-xs">{{ pretty(store.selected) }}</pre>
              </div>
            </main>
            <EmptyState v-else icon="branch" title="Select an orchestration" />
          </div>
        </template>
      </SplitPane>
    </template>

    <template v-else>
      <SplitPane
        class="min-h-0 flex-1"
        storage-key="command-center.orchestrations.adapters.split"
        :initial-first-pct="28"
        :min-first="260"
        :min-second="420"
        collapsible-first
        first-label="Adapters"
        first-icon="box"
        mobile-mode="toggle"
        :mobile-detail-active="!!store.selectedAdapter && !showAdapterList"
      >
        <template #first>
          <aside class="adapter-list panel overflow-auto p-0">
            <div class="adapter-list-header">
              <div class="flex min-w-0 items-center gap-3">
                <span class="adapter-list-icon"><Icon name="box" :size="18" /></span>
                <div class="min-w-0">
                  <p class="adapter-eyebrow">Configured</p>
                  <h3 class="m-0 text-sm font-semibold text-fg">Adapters</h3>
                </div>
              </div>
              <span class="adapter-count" :title="`${store.adapters.length} configured adapters`">
                {{ store.adapters.length }}
              </span>
            </div>

            <div v-if="store.adapters.length" class="adapter-list-items">
              <button
                v-for="adapter in store.adapters"
                :key="adapter.id"
                type="button"
                class="adapter-list-item"
                :class="{ 'is-selected': adapter.id === store.selectedAdapterId }"
                @click="openAdapter(adapter.id)"
              >
                <span class="adapter-mark">{{ adapterMark(adapter.kind) }}</span>
                <span class="min-w-0 flex-1">
                  <span class="flex min-w-0 items-start justify-between gap-2">
                    <span class="truncate font-medium text-fg">{{ adapter.name }}</span>
                    <StatusBadge :status="adapter.enabled" true-label="Live" false-label="Paused" />
                  </span>
                  <span class="mt-1 block truncate text-xs text-fg-muted">
                    {{ adapter.kind }} · revision {{ adapter.current_revision }}
                  </span>
                </span>
              </button>
            </div>
            <EmptyState v-else compact icon="box" title="No adapters configured">
              <button class="btn btn-primary btn-sm" @click="openAdapterForm()">
                <Icon name="plus" :size="15" />
                <span>Create the first adapter</span>
              </button>
            </EmptyState>

            <section class="adapter-catalog" aria-label="Available adapter kinds">
              <div class="adapter-catalog-heading">
                <div>
                  <p class="adapter-eyebrow">Adapter host</p>
                  <h3 class="m-0 text-sm font-semibold text-fg">Available kinds</h3>
                </div>
                <div class="btn-row">
                  <button
                    class="btn btn-ghost btn-icon"
                    type="button"
                    title="Check adapter host health"
                    aria-label="Check adapter host health"
                    @click="checkHost"
                  >
                    <Icon name="info" :size="16" />
                  </button>
                  <button
                    class="btn btn-ghost btn-icon"
                    type="button"
                    title="Reload adapter plugins"
                    aria-label="Reload adapter plugins"
                    @click="reloadHost"
                  >
                    <Icon name="refresh" :size="16" />
                  </button>
                </div>
              </div>
              <div v-if="adapterCatalog.length" class="grid gap-1.5">
                <article
                  v-for="entry in adapterCatalog"
                  :key="`${entry.metadata.kind}:${entry.origin}`"
                  class="adapter-catalog-item"
                  :class="{ 'has-error': !entry.healthy || entry.error }"
                  :title="entry.error || entry.metadata.description || ''"
                >
                  <span class="adapter-catalog-mark">{{ adapterMark(entry.metadata.kind) }}</span>
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center justify-between gap-2">
                      <span class="truncate text-xs font-medium text-fg">
                        {{ entry.metadata.display_name }}
                      </span>
                      <span
                        class="adapter-health-dot"
                        :class="entry.healthy && !entry.error ? 'is-healthy' : 'is-error'"
                        :title="entry.healthy && !entry.error ? 'Healthy' : 'Unavailable'"
                      />
                    </div>
                    <p class="mt-0.5 truncate text-[11px] text-fg-muted">
                      v{{ entry.metadata.version }} · {{ entry.origin }}
                    </p>
                    <p v-if="entry.error" class="mt-1 text-xs text-danger-fg">{{ entry.error }}</p>
                  </div>
                </article>
              </div>
              <p v-else class="m-0 text-xs text-fg-muted">
                No provider kinds are currently loaded.
              </p>
              <details v-if="hostResult" class="adapter-host-details">
                <summary>Host response</summary>
                <pre>{{ pretty(hostResult) }}</pre>
              </details>
            </section>
          </aside>
        </template>

        <template #second>
          <div class="panel details adapter-detail overflow-auto">
            <MobileBackBar label="Back to adapters" @back="showAdapterList = true" />
            <main v-if="store.selectedAdapter" class="grid gap-4">
              <section class="adapter-hero">
                <div class="adapter-hero-top">
                  <div class="flex min-w-0 items-start gap-3">
                    <span class="adapter-hero-mark">{{
                      adapterMark(store.selectedAdapter.kind)
                    }}</span>
                    <div class="min-w-0">
                      <p class="adapter-eyebrow">
                        {{ selectedKind?.display_name || store.selectedAdapter.kind }} adapter
                      </p>
                      <div class="mt-1 flex flex-wrap items-center gap-2">
                        <h2 class="m-0 truncate text-xl font-semibold text-fg">
                          {{ store.selectedAdapter.name }}
                        </h2>
                        <StatusBadge
                          :status="store.selectedAdapter.enabled"
                          true-label="Live"
                          false-label="Paused"
                        />
                      </div>
                      <p class="mt-1 mb-0 text-sm text-fg-muted">
                        {{
                          selectedKind?.description || "Routes provider events into orchestrations."
                        }}
                      </p>
                    </div>
                  </div>
                  <div class="adapter-actions">
                    <button
                      v-if="currentTransport === 'webhook'"
                      class="btn"
                      type="button"
                      @click="copyWebhook"
                    >
                      Copy endpoint
                    </button>
                    <button
                      class="btn"
                      type="button"
                      @click="openAdapterForm(store.selectedAdapter)"
                    >
                      Edit
                    </button>
                    <button
                      class="btn"
                      type="button"
                      @click="openAdapterForm(store.selectedAdapter, true)"
                    >
                      Clone
                    </button>
                    <button class="btn" type="button" @click="toggleSelectedAdapter">
                      {{ store.selectedAdapter.enabled ? "Pause" : "Enable" }}
                    </button>
                    <button
                      class="btn btn-danger"
                      type="button"
                      :disabled="store.selectedAdapter.has_admitted_binding"
                      :title="
                        store.selectedAdapter.has_admitted_binding
                          ? 'Adapters with admitted correlations cannot be deleted.'
                          : 'Delete adapter'
                      "
                      @click="removeSelectedAdapter"
                    >
                      Delete
                    </button>
                  </div>
                </div>
                <div class="adapter-metrics">
                  <MetricCard label="Transport" :value="transportLabel" />
                  <MetricCard
                    label="Current revision"
                    :value="`r${String(store.selectedAdapter.current_revision)}`"
                  />
                  <MetricCard
                    label="Change safety"
                    :value="
                      store.selectedAdapter.has_admitted_binding ? 'Identity locked' : 'Editable'
                    "
                    :value-class="
                      store.selectedAdapter.has_admitted_binding
                        ? 'text-warning-fg'
                        : 'text-success-fg'
                    "
                  />
                </div>
              </section>

              <section v-if="currentTransport === 'webhook'" class="adapter-endpoint">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="adapter-section-icon"><Icon name="key" :size="17" /></span>
                  <div class="min-w-0">
                    <p class="adapter-eyebrow">Delivery endpoint</p>
                    <code class="block truncate text-xs text-fg">{{ webhookPath }}</code>
                  </div>
                </div>
                <button class="btn btn-sm" type="button" @click="copyWebhook">Copy URL</button>
              </section>

              <section v-else-if="store.adapterPollStatus" class="adapter-poll-status">
                <div class="flex items-center gap-2">
                  <span class="adapter-section-icon"><Icon name="clock" :size="17" /></span>
                  <div>
                    <p class="adapter-eyebrow">Polling status</p>
                    <h3 class="m-0 text-sm font-semibold text-fg">Scheduled delivery</h3>
                  </div>
                </div>
                <div class="adapter-poll-grid">
                  <div>
                    <span>Next poll</span>
                    <strong>{{ formatTimestamp(store.adapterPollStatus.next_poll_at) }}</strong>
                  </div>
                  <div>
                    <span>Last success</span>
                    <strong>{{ formatTimestamp(store.adapterPollStatus.last_success_at) }}</strong>
                  </div>
                  <div>
                    <span>Last attempt</span>
                    <strong>{{ formatTimestamp(store.adapterPollStatus.last_attempt_at) }}</strong>
                  </div>
                </div>
                <div v-if="store.adapterPollStatus.last_error" class="adapter-poll-error">
                  <strong>Latest delivery error</strong>
                  <p>{{ store.adapterPollStatus.last_error }}</p>
                </div>
                <details class="adapter-raw-details">
                  <summary>View durable checkpoint</summary>
                  <pre>{{ pretty(store.adapterPollStatus.checkpoint) }}</pre>
                </details>
              </section>

              <p v-if="store.selectedAdapter.has_admitted_binding" class="adapter-safety-note">
                <Icon name="lock" :size="16" />
                Identity extraction and transport are locked because this adapter has admitted a
                correlation. You can still create a new immutable revision for other settings.
              </p>

              <section v-if="selectedKind" class="adapter-overview-grid">
                <article class="adapter-info-card">
                  <p class="adapter-eyebrow">Provider behavior</p>
                  <h3>Capabilities</h3>
                  <div class="adapter-chip-list">
                    <span
                      v-for="capability in selectedKind.capabilities"
                      :key="capability"
                      class="adapter-chip"
                    >
                      {{ capability }}
                    </span>
                    <span v-if="!selectedKind.capabilities.length" class="text-xs text-fg-muted">
                      normalize
                    </span>
                  </div>
                </article>
                <article class="adapter-info-card">
                  <p class="adapter-eyebrow">Normalized events</p>
                  <h3>Event vocabulary</h3>
                  <div class="adapter-chip-list">
                    <span
                      v-for="event in selectedKind.event_names"
                      :key="event"
                      class="adapter-chip"
                    >
                      {{ event }}
                    </span>
                    <span v-if="!selectedKind.event_names.length" class="text-xs text-fg-muted">
                      Provider-defined
                    </span>
                  </div>
                </article>
                <article class="adapter-info-card">
                  <p class="adapter-eyebrow">Routing data</p>
                  <h3>Canonical pointers</h3>
                  <div class="adapter-pointer-list">
                    <code v-for="pointer in selectedKind.canonical_pointers" :key="pointer">{{
                      pointer
                    }}</code>
                    <span
                      v-if="!selectedKind.canonical_pointers.length"
                      class="text-xs text-fg-muted"
                    >
                      Provider-defined
                    </span>
                  </div>
                </article>
              </section>

              <details v-if="selectedKind?.setup_instructions?.length" class="adapter-setup">
                <summary>
                  <span>
                    <span class="adapter-eyebrow">Provider setup</span>
                    <strong>Setup checklist</strong>
                  </span>
                  <span class="text-xs text-fg-muted">
                    {{ selectedKind.setup_instructions.length }} steps
                  </span>
                </summary>
                <ol>
                  <li v-for="instruction in selectedKind.setup_instructions" :key="instruction">
                    {{ instruction }}
                  </li>
                </ol>
              </details>

              <section class="adapter-workspace">
                <nav class="adapter-tabs" aria-label="Adapter detail sections" role="tablist">
                  <button
                    v-for="tab in adapterTabs"
                    :key="tab"
                    type="button"
                    role="tab"
                    :aria-selected="tab === activeAdapterTab"
                    :class="{ 'is-active': tab === activeAdapterTab }"
                    @click="activeAdapterTab = tab"
                  >
                    {{ tab }}
                  </button>
                </nav>

                <section v-if="activeAdapterTab === 'Configuration'" class="adapter-tab-panel">
                  <div class="adapter-tab-heading">
                    <div>
                      <p class="adapter-eyebrow">Immutable revision</p>
                      <h3>Configuration at a glance</h3>
                    </div>
                    <span class="badge status-muted"
                      >r{{ store.selectedAdapter.current_revision }}</span
                    >
                  </div>
                  <div class="adapter-config-grid">
                    <article class="adapter-config-card">
                      <h4>Connection settings</h4>
                      <dl v-if="currentConfigurationEntries.length">
                        <div v-for="[key, value] in currentConfigurationEntries" :key="key">
                          <dt>{{ humanizeKey(key) }}</dt>
                          <dd>{{ formatConfigValue(value) }}</dd>
                        </div>
                      </dl>
                      <p v-else class="text-sm text-fg-muted">No connection settings recorded.</p>
                    </article>
                    <article class="adapter-config-card">
                      <h4>Identity extraction</h4>
                      <dl v-if="currentIdentityEntries.length">
                        <div v-for="[key, value] in currentIdentityEntries" :key="key">
                          <dt>{{ humanizeKey(key) }}</dt>
                          <dd>{{ formatConfigValue(value) }}</dd>
                        </div>
                      </dl>
                      <p v-else class="text-sm text-fg-muted">No identity rules recorded.</p>
                    </article>
                    <article class="adapter-config-card">
                      <h4>Secret bindings</h4>
                      <dl v-if="currentSecretBindingEntries.length">
                        <div v-for="[key, value] in currentSecretBindingEntries" :key="key">
                          <dt>{{ humanizeKey(key) }}</dt>
                          <dd>{{ value }}</dd>
                        </div>
                      </dl>
                      <p v-else class="text-sm text-fg-muted">
                        This revision has no secret bindings.
                      </p>
                    </article>
                  </div>
                  <details class="adapter-raw-details">
                    <summary>View raw revision data</summary>
                    <pre>{{ pretty(currentAdapterRevision) }}</pre>
                  </details>
                </section>

                <section v-else-if="activeAdapterTab === 'Revisions'" class="adapter-tab-panel">
                  <div class="adapter-tab-heading">
                    <div>
                      <p class="adapter-eyebrow">Revision history</p>
                      <h3>Immutable configuration timeline</h3>
                    </div>
                    <span class="text-xs text-fg-muted">
                      {{ store.adapterRevisions.length }} revision{{
                        store.adapterRevisions.length === 1 ? "" : "s"
                      }}
                    </span>
                  </div>
                  <div class="adapter-revision-list">
                    <article
                      v-for="revision in store.adapterRevisions"
                      :key="revision.id"
                      class="adapter-revision-card"
                      :class="{
                        'is-current': revision.revision === store.selectedAdapter.current_revision,
                      }"
                    >
                      <div class="flex flex-wrap items-start justify-between gap-2">
                        <div>
                          <div class="flex items-center gap-2">
                            <h4>Revision {{ revision.revision }}</h4>
                            <span
                              v-if="revision.revision === store.selectedAdapter.current_revision"
                              class="badge status-succeeded"
                            >
                              Current
                            </span>
                          </div>
                          <p>
                            {{
                              revision.transport === "webhook"
                                ? "Webhook delivery"
                                : "Polling delivery"
                            }}
                            · provider v{{ revision.kind_version }}
                          </p>
                        </div>
                        <span class="text-xs text-fg-muted">{{
                          formatTimestamp(revision.created_at)
                        }}</span>
                      </div>
                      <div class="adapter-revision-meta">
                        <span
                          >{{ Object.keys(jsonObject(revision.configuration)).length }} connection
                          settings</span
                        >
                        <span
                          >{{ Object.keys(revision.secret_bindings).length }} secret bindings</span
                        >
                        <span>{{ revision.actor_id || "System" }}</span>
                      </div>
                      <details class="adapter-raw-details">
                        <summary>View revision data</summary>
                        <pre>{{ pretty(revision) }}</pre>
                      </details>
                    </article>
                  </div>
                </section>

                <section v-else class="adapter-tab-panel">
                  <div class="adapter-tab-heading">
                    <div>
                      <p class="adapter-eyebrow">Dry run</p>
                      <h3>Test an incoming delivery</h3>
                    </div>
                    <span class="text-xs text-fg-muted">No event is persisted or routed.</span>
                  </div>
                  <div class="adapter-test-inputs">
                    <label>
                      <span>Request headers</span>
                      <small>JSON object with string values</small>
                      <textarea v-model="testHeaders" class="min-h-28" spellcheck="false" />
                    </label>
                    <label>
                      <span>Request body</span>
                      <small>Paste the provider payload exactly as received</small>
                      <textarea v-model="testBody" class="min-h-48" spellcheck="false" />
                    </label>
                  </div>
                  <button class="btn btn-primary" type="button" @click="runTest">
                    <Icon name="play" :size="16" />
                    Verify and preview routing
                  </button>
                  <section v-if="testResult" class="adapter-test-results">
                    <div
                      class="adapter-test-summary"
                      :class="testResult.verified ? 'is-verified' : 'is-rejected'"
                    >
                      <Icon :name="testResult.verified ? 'check' : 'alert'" :size="18" />
                      <div>
                        <strong>{{
                          testResult.verified ? "Delivery verified" : "Delivery rejected"
                        }}</strong>
                        <p>{{ testResult.events.length }} normalized event(s) ready for preview</p>
                      </div>
                    </div>
                    <ul v-if="testResult.errors.length" class="adapter-test-errors">
                      <li v-for="error in testResult.errors" :key="error">{{ error }}</li>
                    </ul>
                    <article
                      v-for="preview in testResult.previews"
                      :key="preview.delivery_id"
                      class="adapter-preview-card"
                    >
                      <div class="flex flex-wrap items-start justify-between gap-2">
                        <div>
                          <p class="adapter-eyebrow">{{ preview.lifecycle }} lifecycle</p>
                          <h4>{{ preview.event_type }}</h4>
                          <p>{{ preview.scope }}/{{ preview.correlation_key }}</p>
                        </div>
                        <span class="badge status-muted"
                          >{{ preview.pipeline_matches.length }} pipelines</span
                        >
                      </div>
                      <ul v-if="preview.validation_errors.length" class="adapter-preview-warnings">
                        <li v-for="error in preview.validation_errors" :key="error">{{ error }}</li>
                      </ul>
                      <div
                        v-for="match in preview.pipeline_matches"
                        :key="match.pipeline_id"
                        class="adapter-route-preview"
                      >
                        <div class="flex flex-wrap items-center justify-between gap-2">
                          <strong>{{ match.pipeline_name }}</strong>
                          <span class="text-xs text-fg-muted">
                            {{ match.managed ? "Managed pipeline" : "Unmanaged pipeline" }}
                          </span>
                        </div>
                        <p>
                          Routes:
                          {{ match.routes.map((route) => route.action).join(", ") || "none" }}
                        </p>
                        <p>Candidate intents: {{ match.candidate_intents.join(", ") || "none" }}</p>
                        <p v-if="match.winner">
                          <strong>Selected intent:</strong> {{ match.winner }}
                          <template v-if="match.suppressed_intents.length">
                            · suppressed {{ match.suppressed_intents.join(", ") }}
                          </template>
                        </p>
                        <details class="adapter-raw-details">
                          <summary>View matched route details</summary>
                          <pre>{{ pretty(match.routes) }}</pre>
                        </details>
                      </div>
                    </article>
                    <details class="adapter-raw-details">
                      <summary>View raw normalized response</summary>
                      <pre>{{ pretty(testResult) }}</pre>
                    </details>
                  </section>
                </section>
              </section>
            </main>
            <EmptyState v-else icon="box" title="Select an adapter">
              <button class="btn btn-primary" @click="openAdapterForm()">
                <Icon name="plus" />
                <span>New adapter</span>
              </button>
            </EmptyState>
          </div>
        </template>
      </SplitPane>
    </template>

    <Modal
      v-if="intentName"
      :title="`Dispatch ${intentName}`"
      width="min(520px, 100%)"
      @close="intentName = null"
    >
      <form id="orchestration-intent-form" class="grid gap-3" @submit.prevent="submitIntent">
        <label>Reason<textarea v-model="reason" required class="min-h-24" /></label>
        <label
          >Payload JSON<textarea v-model="intentPayload" class="min-h-28 font-mono text-xs" />
        </label>
        <p v-if="intentPayloadError" class="m-0 text-sm text-danger-fg">{{ intentPayloadError }}</p>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="intentName = null">Cancel</button>
        <button class="btn btn-primary" type="submit" form="orchestration-intent-form">
          Dispatch
        </button>
      </template>
    </Modal>
    <Modal
      v-if="requeueOpen"
      title="Requeue next generation"
      width="min(520px, 100%)"
      @close="requeueOpen = false"
    >
      <p class="m-0 text-xs text-fg-muted">
        The next generation snapshots the current immutable pipeline and adapter revisions.
      </p>
      <form id="orchestration-requeue-form" class="grid gap-3" @submit.prevent="submitRequeue">
        <label>Reason<textarea v-model="reason" required class="min-h-24" /></label>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="requeueOpen = false">Cancel</button>
        <button class="btn btn-primary" type="submit" form="orchestration-requeue-form">
          Requeue
        </button>
      </template>
    </Modal>
    <Modal
      v-if="resolvingOperation"
      :title="`Resolve ${resolvingOperation.provider}.${resolvingOperation.action}`"
      width="min(620px, 100%)"
      @close="resolvingOperation = null"
    >
      <p class="m-0 text-xs text-fg-muted">{{ resolution }} · {{ resolvingOperation.semantics }}</p>
      <form
        id="orchestration-resolution-form"
        class="grid gap-3"
        @submit.prevent="submitResolution"
      >
        <label>Reason<textarea v-model="resolutionReason" required class="min-h-20" /></label>
        <label
          >Receipt JSON<textarea v-model="resolutionReceipt" class="min-h-28 font-mono text-xs" />
        </label>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="resolvingOperation = null">Cancel</button>
        <button class="btn btn-primary" type="submit" form="orchestration-resolution-form">
          Apply resolution
        </button>
      </template>
    </Modal>
    <Modal
      v-if="adapterFormOpen"
      :title="editingAdapterId ? 'Edit adapter' : 'New adapter'"
      width="min(820px, 100%)"
      @close="adapterFormOpen = false"
    >
      <form id="adapter-form" class="grid gap-3" @submit.prevent="saveAdapter">
        <div class="mt-3 grid gap-3 md:grid-cols-3">
          <label class="text-sm"
            >Name<input v-model="adapterForm.name" required class="mt-1 w-full" /></label
          ><label class="text-sm"
            >Kind<select
              v-model="adapterForm.kind"
              required
              class="mt-1 w-full"
              :disabled="!!editingAdapterId"
              @change="initializeKind"
            >
              <option value="" disabled>Select a loaded kind</option>
              <option v-for="kind in store.adapterKinds" :key="kind.kind" :value="kind.kind">
                {{ kind.display_name }} v{{ kind.version }}
              </option>
            </select></label
          ><label class="text-sm"
            >Transport<select
              v-model="adapterForm.transport"
              class="mt-1 w-full"
              :disabled="identityLocked"
            >
              <option value="webhook">Webhook</option>
              <option
                v-if="adapterForm.kind === 'github' || adapterForm.kind === 'jira'"
                value="polling"
              >
                Polling
              </option>
            </select></label
          >
        </div>
        <div v-if="formKind" class="mt-4 grid gap-3">
          <p class="text-sm text-fg-muted">{{ formKind.description }}</p>
          <template v-if="adapterForm.transport === 'polling'"
            ><label class="text-sm"
              >Poll interval (seconds)<input
                v-model.number="adapterForm.configuration.poll_interval_seconds"
                type="number"
                min="30"
                max="3600"
                required
                class="mt-1 w-full" /></label
            ><label v-if="adapterForm.kind === 'github'" class="text-sm"
              >Repositories (JSON array)<textarea
                v-model="pollRepositories"
                class="mt-1 min-h-20 w-full font-mono text-xs"
              /></label
            ><template v-if="adapterForm.kind === 'jira'"
              ><label class="text-sm"
                >Jira instance identity<input
                  v-model="adapterForm.configuration.instance_id"
                  required
                  class="mt-1 w-full"
                  placeholder="acme.atlassian.net" /></label
              ><label class="text-sm"
                >Jira base URL<input
                  v-model="adapterForm.configuration.base_url"
                  required
                  class="mt-1 w-full" /></label
              ><label class="text-sm"
                >Jira account email<input
                  v-model="adapterForm.configuration.email"
                  required
                  class="mt-1 w-full" /></label
              ><label class="text-sm"
                >JQL<input
                  v-model="adapterForm.configuration.jql"
                  required
                  class="mt-1 w-full" /></label></template
            ><label class="text-sm"
              >{{ adapterForm.kind === "github" ? "access_token" : "api_token" }} Secret<select
                v-model="
                  adapterForm.secret_bindings[
                    adapterForm.kind === 'github' ? 'access_token' : 'api_token'
                  ]
                "
                required
                class="mt-1 w-full"
              >
                <option value="">Select stored Secret</option>
                <option v-for="secret in selectableSecrets" :key="secret.id" :value="secret.id">
                  {{ secret.scope }}/{{ secret.name }}
                </option>
              </select></label
            ></template
          ><template v-else
            ><label v-for="field in configurationFields" :key="field.name" class="text-sm"
              ><span>{{ field.name }}<template v-if="field.required"> *</template></span
              ><TypedValueEditor
                class="mt-1"
                :model-value="adapterForm.configuration[field.name]"
                :ty="field.value_type"
                :allow-expressions="false"
                @update:model-value="updateConfigField(field.name, $event)"
              /><small v-if="field.description" class="mt-1 block text-fg-muted">{{
                field.description
              }}</small></label
            ><label v-for="field in secretFields" :key="field.name" class="text-sm"
              >{{ field.name }} Secret<template v-if="field.required"> *</template
              ><select
                v-model="adapterForm.secret_bindings[field.name]"
                class="mt-1 w-full"
                :required="field.required"
              >
                <option value="">Select stored Secret</option>
                <option v-for="secret in selectableSecrets" :key="secret.id" :value="secret.id">
                  {{ secret.scope }}/{{ secret.name }}
                </option>
              </select></label
            ></template
          ><label class="text-sm"
            >Identity extraction JSON<textarea
              v-model="identityText"
              class="mt-1 min-h-28 w-full font-mono text-xs"
              :disabled="identityLocked"
            />
          </label>
        </div>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="adapterFormOpen = false">Cancel</button>
        <button class="btn btn-primary" type="submit" form="adapter-form">
          Save immutable revision
        </button>
      </template>
    </Modal>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, shallowRef, watch } from "vue";
import type {
  AdapterDefinition,
  AdapterKindCatalogEntry,
  AdapterKindMetadata,
  AdapterRevision,
  ExternalOperation,
  JsonValue,
  OrchestrationEvidence,
  PipelineRunDetail,
  WorkspaceLease,
} from "../../core/domain/models";
import {
  fetchAdapterHealth,
  fetchAdapterKinds,
  reloadAdapterHost,
} from "../../core/services/orchestrations";
import { useAppStore } from "../adapters/pinia/app";
import { useOrchestrationsStore } from "../adapters/pinia/orchestrations";
import { usePipelineRunsStore } from "../adapters/pinia/pipeline-runs";
import { useSecretsStore } from "../adapters/pinia/secrets";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import PipelineCanvas from "../components/pipeline/PipelineCanvas.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import Modal from "../components/shared/Modal.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import TypedValueEditor from "../components/shared/TypedValueEditor.vue";
import { downloadTextFile } from "../adapters/browser/files";

const store = useOrchestrationsStore();
const app = useAppStore();
const pipelineRuns = usePipelineRunsStore();
const secrets = useSecretsStore();
const workflows = useWorkflowsStore();
const modes = ["Instances", "Adapters"] as const;
type Mode = (typeof modes)[number];
const mode = ref<Mode>("Instances");
const statuses = [
  "pending",
  "running",
  "waiting",
  "suspended",
  "completed",
  "failed",
  "terminated",
];
const instanceTabs = [
  "Timeline",
  "Epochs",
  "Evidence",
  "Resources",
  "Budgets",
  "Operations",
  "Workspaces",
  "Aliases",
  "Commands",
  "Raw",
];
const adapterTabs = ["Configuration", "Revisions", "Test"];
const activeInstanceTab = ref("Timeline");
// mobile master-detail: the store always keeps a selection, so "back" is a local pane swap.
const showInstanceList = ref(false);
const showAdapterList = ref(false);
const activeAdapterTab = ref("Configuration");
const filters = reactive({
  status: "",
  scope: "",
  correlation_key: "",
  pipeline_id: "",
  adapter_id: "",
});

function openBinding(id: string) {
  showInstanceList.value = false;
  void store.select(id);
}

function openAdapter(id: string) {
  showAdapterList.value = false;
  void store.selectAdapter(id);
}

const intentName = ref<string | null>(null);
const intentPayload = ref("{}");
const intentPayloadError = ref<string | null>(null);
const requeueOpen = ref(false);
const reason = ref("");
const aliasSource = ref("");
const aliasScope = ref("");
const aliasCorrelation = ref("");
const resolvingOperation = ref<ExternalOperation | null>(null);
const resolution = ref<"succeeded" | "failed" | "retry">("succeeded");
const resolutionReason = ref("");
const resolutionReceipt = ref("null");
const hostResult = ref<unknown>(null);
const adapterCatalog = shallowRef<AdapterKindCatalogEntry[]>([]);
const testHeaders = ref("{}");
const testBody = ref("{}");
interface AdapterTestRoute {
  action: string;
  intent?: string | null;
  predicates: unknown[];
}
interface AdapterTestPipelineMatch {
  pipeline_id: string;
  pipeline_name: string;
  managed: boolean;
  routes: AdapterTestRoute[];
  candidate_intents: string[];
  winner?: string | null;
  suppressed_intents: string[];
}
interface AdapterEventPreview {
  delivery_id: string;
  scope: string;
  correlation_key: string;
  event_type: string;
  lifecycle: string;
  pipeline_matches: AdapterTestPipelineMatch[];
  validation_errors: string[];
}
interface AdapterTestResult {
  verified: boolean;
  events: unknown[];
  errors: string[];
  previews: AdapterEventPreview[];
}
const testResult = ref<AdapterTestResult | null>(null);
const adapterFormOpen = ref(false);
const editingAdapterId = ref<string | null>(null);
const identityText = ref("{}");
interface AdapterFormState {
  name: string;
  kind: string;
  transport: "webhook" | "polling";
  configuration: Record<string, JsonValue>;
  secret_bindings: Record<string, string>;
}
const adapterForm = reactive<AdapterFormState>({
  name: "",
  kind: "",
  transport: "webhook",
  configuration: {},
  secret_bindings: {},
});

const selectedKind = computed<AdapterKindMetadata | undefined>(() =>
  store.adapterKinds.find((kind) => kind.kind === store.selectedAdapter?.kind),
);
const formKind = computed<AdapterKindMetadata | undefined>(() =>
  store.adapterKinds.find((kind) => kind.kind === adapterForm.kind),
);
const configurationFields = computed(
  () => formKind.value?.fields.filter((field) => !field.secret) ?? [],
);
const pollRepositories = ref("[]");
const secretFields = computed(() => formKind.value?.fields.filter((field) => field.secret) ?? []);
const selectableSecrets = computed(() =>
  secrets.secretEntries.filter((secret) => Boolean(secret.id)),
);
const currentAdapterRevision = computed<AdapterRevision | undefined>(
  () =>
    store.adapterRevisions.find(
      (revision) => revision.revision === store.selectedAdapter?.current_revision,
    ) ?? store.adapterRevisions[0],
);
const currentTransport = computed(() => currentAdapterRevision.value?.transport ?? "webhook");
const transportLabel = computed(() =>
  currentTransport.value === "webhook" ? "Webhook" : "Polling",
);
const currentConfigurationEntries = computed(() =>
  Object.entries(jsonObject(currentAdapterRevision.value?.configuration)),
);
const currentIdentityEntries = computed(() =>
  Object.entries(jsonObject(currentAdapterRevision.value?.identity_configuration)),
);
const currentSecretBindingEntries = computed(() =>
  Object.entries(currentAdapterRevision.value?.secret_bindings ?? {}),
);
const webhookPath = computed(() =>
  store.selectedAdapter ? `/webhooks/orchestration/${store.selectedAdapter.endpoint_identity}` : "",
);
const identityLocked = computed(() =>
  Boolean(editingAdapterId.value && store.selectedAdapter?.has_admitted_binding),
);
const isSelectedTerminal = computed(() =>
  Boolean(store.selected && ["completed", "failed", "terminated"].includes(store.selected.status)),
);
const instanceChips = computed(() =>
  store.selected
    ? [
        `generation ${String(store.selected.generation)}`,
        `epoch ${String(store.selected.current_epoch)}`,
        `phase ${store.selected.current_phase ?? "—"}`,
        `attempt ${String(store.selected.current_attempt)}`,
        `CAS ${String(store.selected.version)}`,
        `pipeline revision ${String(store.selected.pipeline_revision)}`,
        ...(store.selected.adapter_id
          ? [`adapter revision ${String(store.selected.adapter_revision ?? "—")}`]
          : []),
      ]
    : [],
);
const currentEpoch = computed(() =>
  store.epochs.find((epoch) => epoch.epoch === store.selected?.current_epoch),
);
const currentEpochRunId = computed(() => currentEpoch.value?.pipeline_run_id ?? null);
const currentEpochDetail = computed<PipelineRunDetail | null>(() => {
  const detail: PipelineRunDetail | null = pipelineRuns.detail;

  return detail?.run.id === currentEpochRunId.value ? detail : null;
});

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function formatTimestamp(value?: string | null): string {
  return value ? new Date(value).toLocaleString() : "Never";
}

function adapterMark(kind: string): string {
  const compact = kind.replace(/[^a-z0-9]/gi, "").slice(0, 2);

  return compact ? compact.toUpperCase() : "AD";
}

function humanizeKey(value: string): string {
  return value
    .replace(/[_-]+/g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/^./, (character) => character.toUpperCase());
}

function formatConfigValue(value: JsonValue): string {
  if (value === null) {
    return "Not set";
  }

  if (typeof value === "boolean") {
    return value ? "Enabled" : "Disabled";
  }

  if (typeof value === "string") {
    return value || "Not set";
  }

  const rendered = JSON.stringify(value);

  return rendered.length > 150 ? `${rendered.slice(0, 147)}…` : rendered;
}

function safeFileSegment(value: string): string {
  return value.replace(/[^a-zA-Z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "evidence";
}

function downloadJson(fileName: string, value: unknown): void {
  downloadTextFile(fileName, `${pretty(value)}\n`, "application/json");
}

function downloadEvidence(item: OrchestrationEvidence): void {
  downloadJson(`${safeFileSegment(item.kind)}-${safeFileSegment(item.id)}.json`, item);
}

function downloadAllEvidence(): void {
  const correlation = store.selected?.correlation_key ?? "orchestration";

  downloadJson(`${safeFileSegment(correlation)}-evidence.json`, store.evidence);
}

function downloadWorkspaceEvidence(workspace: WorkspaceLease): void {
  downloadJson(
    `workspace-${safeFileSegment(workspace.scope)}-attempt-${String(workspace.attempt)}.json`,
    {
      workspace_id: workspace.id,
      admission_id: workspace.admission_id,
      generation: workspace.generation,
      scope: workspace.scope,
      attempt: workspace.attempt,
      status: workspace.status,
      local_key: workspace.local_key,
      evidence: workspace.evidence,
    },
  );
}

function jsonObject(value: JsonValue | undefined): Record<string, JsonValue> {
  return value && typeof value === "object" && !Array.isArray(value) ? { ...value } : {};
}

function parseJson(value: string): unknown {
  const parsed: unknown = JSON.parse(value);

  return parsed;
}

function refreshInstances(): void {
  void store.refresh(Object.fromEntries(Object.entries(filters).filter(([, value]) => value)));
}

async function refreshAdapters(): Promise<void> {
  const [, , catalog] = await Promise.all([
    store.refreshAdapters(),
    secrets.refreshSecrets(),
    fetchAdapterKinds(),
  ]);
  adapterCatalog.value = catalog;
}

function switchMode(next: Mode): void {
  mode.value = next;

  if (next === "Instances") {
    refreshInstances();
  } else {
    void refreshAdapters();
  }
}

function openIntent(name: string): void {
  intentName.value = name;
  reason.value = "";
  intentPayload.value = "{}";
  intentPayloadError.value = null;
}

function openRequeue(): void {
  reason.value = "";
  requeueOpen.value = true;
}

async function submitIntent(): Promise<void> {
  if (!intentName.value || !reason.value.trim()) {
    return;
  }

  let payload: unknown;

  try {
    payload = parseJson(intentPayload.value || "{}");
  } catch (cause) {
    intentPayloadError.value =
      cause instanceof Error ? cause.message : "Payload must be valid JSON.";
    return;
  }

  await store.dispatch(intentName.value, reason.value.trim(), payload);
  intentName.value = null;
}

async function openPipelineRun(id: string): Promise<void> {
  await pipelineRuns.selectRun(id);
  app.activeTab = "PipelineRuns";
}

function openWorkflowRun(id: string): void {
  const run = currentEpochDetail.value?.members.find((member) => member.id === id);

  if (run) {
    void workflows.selectWorkflowRun(run);
    app.activeTab = "Runs";
  }
}

async function submitRequeue(): Promise<void> {
  if (!reason.value.trim()) {
    return;
  }

  await store.requeue(reason.value.trim());
  requeueOpen.value = false;
}

async function submitAlias(): Promise<void> {
  const source = aliasSource.value.trim();
  const scope = aliasScope.value.trim();
  const correlation = aliasCorrelation.value.trim();

  if (!source || !scope || !correlation) {
    return;
  }

  await store.addAlias(source, scope, correlation);
  aliasSource.value = "";
  aliasScope.value = "";
  aliasCorrelation.value = "";
}

function openResolution(operation: ExternalOperation, next: typeof resolution.value): void {
  resolvingOperation.value = operation;
  resolution.value = next;
  resolutionReason.value = "";
  resolutionReceipt.value = "null";
}

async function submitResolution(): Promise<void> {
  if (!resolvingOperation.value || !resolutionReason.value.trim()) {
    return;
  }

  const receipt = parseJson(resolutionReceipt.value || "null");

  await store.resolveOperation(
    resolvingOperation.value,
    resolution.value,
    resolutionReason.value.trim(),
    receipt,
  );
  resolvingOperation.value = null;
}

async function checkHost(): Promise<void> {
  hostResult.value = await fetchAdapterHealth();
}

async function reloadHost(): Promise<void> {
  hostResult.value = await reloadAdapterHost();
  await refreshAdapters();
}

async function copyWebhook(): Promise<void> {
  await navigator.clipboard.writeText(webhookPath.value);
}

async function toggleSelectedAdapter(): Promise<void> {
  if (store.selectedAdapter) {
    await store.toggleAdapter(store.selectedAdapter);
  }
}

async function removeSelectedAdapter(): Promise<void> {
  if (store.selectedAdapter) {
    await store.removeAdapter(store.selectedAdapter);
  }
}

function toBase64(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";

  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  return btoa(binary);
}

async function runTest(): Promise<void> {
  if (!store.selectedAdapter) {
    return;
  }

  const parsed = parseJson(testHeaders.value || "{}");

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("Headers must be a JSON object");
  }

  const headers: Record<string, string> = {};

  for (const [name, value] of Object.entries(parsed)) {
    if (typeof value !== "string") {
      throw new Error(`Header ${name} must be a string`);
    }

    headers[name] = value;
  }

  testResult.value = (await store.runAdapterTest(
    store.selectedAdapter.id,
    headers,
    toBase64(testBody.value),
  )) as AdapterTestResult;
}

function initializeKind(): void {
  adapterForm.configuration = {};
  adapterForm.secret_bindings = {};
  pollRepositories.value = "[]";

  if (adapterForm.kind !== "github" && adapterForm.kind !== "jira") {
    adapterForm.transport = "webhook";
  }

  for (const field of formKind.value?.fields ?? []) {
    if (!field.secret) {
      adapterForm.configuration[field.name] = field.default as JsonValue;
    }
  }
}

function openAdapterForm(adapter?: AdapterDefinition, clone = false): void {
  const revision: AdapterRevision | undefined = adapter ? currentAdapterRevision.value : undefined;
  const firstKind = store.adapterKinds.at(0);

  editingAdapterId.value = adapter && !clone ? adapter.id : null;
  adapterForm.name = adapter ? `${adapter.name}${clone ? " copy" : ""}` : "";
  adapterForm.kind = adapter ? adapter.kind : firstKind ? firstKind.kind : "";
  adapterForm.transport = revision?.transport ?? "webhook";
  adapterForm.configuration = revision ? jsonObject(revision.configuration) : {};
  adapterForm.secret_bindings = revision ? { ...revision.secret_bindings } : {};
  pollRepositories.value = pretty(adapterForm.configuration.repositories ?? []);
  identityText.value = pretty(revision?.identity_configuration ?? {});

  if (!revision) {
    initializeKind();
  }

  adapterFormOpen.value = true;
}

function updateConfigField(name: string, value: unknown): void {
  adapterForm.configuration[name] = (value ?? null) as JsonValue;
}

async function saveAdapter(): Promise<void> {
  const kind = formKind.value;

  if (!kind) {
    return;
  }

  const identity = parseJson(identityText.value || "{}");
  const configuration = { ...adapterForm.configuration };

  if (adapterForm.transport === "polling" && adapterForm.kind === "github") {
    configuration.repositories = parseJson(pollRepositories.value || "[]") as JsonValue;
  }

  const bindings = Object.fromEntries(
    Object.entries(adapterForm.secret_bindings).filter(([, value]) => value),
  );

  await store.saveAdapter(
    {
      name: adapterForm.name.trim(),
      kind: kind.kind,
      kind_version: kind.version,
      transport: adapterForm.transport,
      configuration,
      secret_bindings: bindings,
      identity_configuration: identity,
      ...(editingAdapterId.value && store.selectedAdapter
        ? { expected_revision: store.selectedAdapter.current_revision }
        : {}),
    },
    editingAdapterId.value ?? undefined,
  );
  adapterFormOpen.value = false;
}

watch(
  currentEpochRunId,
  (id) => {
    if (id && pipelineRuns.selectedRunId !== id) {
      void pipelineRuns.selectRun(id);
    }
  },
  { immediate: true },
);

onMounted(refreshInstances);
</script>

<style scoped>
.adapter-list {
  gap: 0;
  background: var(--surface);
}

.adapter-list-header,
.adapter-catalog-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-5);
}

.adapter-list-header {
  border-bottom: 1px solid var(--border-subtle);
}

.adapter-list-icon,
.adapter-section-icon {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border));
  border-radius: var(--radius);
  background: var(--accent-soft);
  color: var(--accent-text);
}

.adapter-eyebrow {
  margin: 0;
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  line-height: 1.2;
  text-transform: uppercase;
}

.adapter-count {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  min-width: 26px;
  height: 26px;
  padding: 0 7px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.adapter-list-items {
  display: grid;
  gap: 3px;
  padding: var(--space-2);
}

.adapter-list-item {
  display: flex;
  width: 100%;
  align-items: center;
  gap: var(--space-3);
  border: 1px solid transparent;
  border-radius: var(--radius);
  background: transparent;
  padding: 9px;
  color: inherit;
  text-align: left;
  transition:
    background-color 150ms ease,
    border-color 150ms ease,
    box-shadow 150ms ease,
    transform 150ms ease;
}

.adapter-list-item:hover {
  border-color: var(--border-subtle);
  background: var(--surface-hover);
}

.adapter-list-item:active {
  transform: scale(0.99);
}

.adapter-list-item.is-selected,
.adapter-list-item.is-selected:hover {
  border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  background: var(--accent-soft);
  box-shadow: inset 3px 0 0 var(--accent);
}

.adapter-mark,
.adapter-catalog-mark,
.adapter-hero-mark {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-sunken);
  color: var(--accent-text);
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.03em;
}

.adapter-mark {
  width: 32px;
  height: 32px;
}

.adapter-list-item.is-selected .adapter-mark {
  border-color: color-mix(in srgb, var(--accent) 25%, var(--border));
  background: var(--surface);
}

.adapter-catalog {
  display: grid;
  gap: var(--space-2);
  margin-top: auto;
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-catalog-heading {
  padding: var(--space-1) var(--space-1) var(--space-2);
}

.adapter-catalog-item {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 7px;
}

.adapter-catalog-item.has-error {
  border-color: color-mix(in srgb, var(--danger-fg) 28%, var(--border));
}

.adapter-catalog-mark {
  width: 24px;
  height: 24px;
  border-radius: var(--radius-sm);
  font-size: 9px;
}

.adapter-health-dot {
  width: 7px;
  height: 7px;
  flex: 0 0 auto;
  border-radius: 50%;
}

.adapter-health-dot.is-healthy {
  background: var(--success-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--success-fg) 14%, transparent);
}

.adapter-health-dot.is-error {
  background: var(--danger-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--danger-fg) 14%, transparent);
}

.adapter-host-details,
.adapter-raw-details {
  color: var(--text-muted);
  font-size: 12px;
}

.adapter-host-details summary,
.adapter-raw-details summary {
  cursor: pointer;
  list-style: none;
}

.adapter-host-details summary::-webkit-details-marker,
.adapter-raw-details summary::-webkit-details-marker {
  display: none;
}

.adapter-host-details summary::before,
.adapter-raw-details summary::before {
  display: inline-block;
  content: "›";
  margin-right: 5px;
  color: var(--accent-text);
  font-size: 15px;
  line-height: 0.7;
  transition: transform 150ms ease;
}

.adapter-host-details[open] summary::before,
.adapter-raw-details[open] summary::before {
  transform: rotate(90deg);
}

.adapter-host-details pre,
.adapter-raw-details pre {
  max-height: 260px;
  margin: var(--space-2) 0 0;
  overflow: auto;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  background: var(--surface-sunken);
  padding: var(--space-3);
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.55;
}

.adapter-detail {
  gap: var(--space-4);
  padding: var(--space-4);
}

.adapter-hero {
  display: grid;
  gap: var(--space-4);
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--accent) 20%, var(--border));
  border-radius: var(--radius-lg);
  background:
    radial-gradient(
      circle at top right,
      color-mix(in srgb, var(--accent) 12%, transparent),
      transparent 42%
    ),
    var(--surface);
  padding: var(--space-5);
}

.adapter-hero-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.adapter-hero-mark {
  width: 42px;
  height: 42px;
  border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  background: var(--accent-soft);
  font-size: 13px;
}

.adapter-actions {
  display: flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.adapter-metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
}

.adapter-endpoint {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-endpoint code {
  font-family: var(--font-mono);
}

.adapter-poll-status {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-4);
}

.adapter-poll-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
}

.adapter-poll-grid > div {
  display: grid;
  gap: 3px;
  border-left: 2px solid var(--accent);
  background: var(--surface);
  padding: var(--space-3);
}

.adapter-poll-grid span {
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-poll-grid strong {
  color: var(--text);
  font-size: 13px;
  line-height: 1.35;
}

.adapter-poll-error {
  border: 1px solid color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  border-radius: var(--radius-sm);
  background: var(--danger-bg);
  padding: var(--space-3);
  color: var(--danger-fg);
  font-size: 12px;
}

.adapter-poll-error p {
  margin: 4px 0 0;
}

.adapter-safety-note {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  margin: 0;
  border: 1px solid color-mix(in srgb, var(--warning-fg) 30%, var(--border));
  border-radius: var(--radius);
  background: var(--warning-bg);
  padding: var(--space-3);
  color: var(--warning-fg);
  font-size: 12px;
  line-height: 1.45;
}

.adapter-overview-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
}

.adapter-info-card {
  display: grid;
  align-content: start;
  gap: var(--space-2);
  min-width: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: var(--space-3);
}

.adapter-info-card h3,
.adapter-tab-heading h3 {
  margin: 0;
  color: var(--text);
  font-size: 13px;
  font-weight: 700;
}

.adapter-chip-list,
.adapter-pointer-list {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.adapter-chip {
  display: inline-flex;
  align-items: center;
  max-width: 100%;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 2px 7px;
  color: var(--text-subtle);
  font-size: 11px;
  line-height: 1.4;
}

.adapter-pointer-list code {
  max-width: 100%;
  overflow: hidden;
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.adapter-setup {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.adapter-setup summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  cursor: pointer;
  list-style: none;
  padding: var(--space-3);
}

.adapter-setup summary::-webkit-details-marker {
  display: none;
}

.adapter-setup strong {
  display: block;
  margin-top: 2px;
  color: var(--text);
  font-size: 13px;
}

.adapter-setup ol {
  display: grid;
  gap: var(--space-2);
  margin: 0;
  border-top: 1px solid var(--border-subtle);
  padding: var(--space-3) var(--space-5) var(--space-4) 32px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.adapter-workspace {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  background: var(--surface);
}

.adapter-tabs {
  display: flex;
  gap: 2px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--surface-sunken);
  padding: 0 var(--space-2);
}

.adapter-tabs button {
  position: relative;
  border: 0;
  border-radius: 0;
  background: transparent;
  padding: 10px var(--space-3);
  color: var(--text-muted);
  font-size: 13px;
}

.adapter-tabs button:hover {
  background: transparent;
  color: var(--text);
}

.adapter-tabs button::after {
  position: absolute;
  right: var(--space-3);
  bottom: -1px;
  left: var(--space-3);
  height: 2px;
  content: "";
  background: transparent;
}

.adapter-tabs button.is-active {
  color: var(--accent-text);
  font-weight: 700;
}

.adapter-tabs button.is-active::after {
  background: var(--accent);
}

.adapter-tab-panel {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-4);
}

.adapter-tab-heading {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-3);
}

.adapter-config-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-2);
}

.adapter-config-card {
  min-width: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-config-card h4,
.adapter-revision-card h4,
.adapter-preview-card h4 {
  margin: 0;
  color: var(--text);
  font-size: 13px;
  font-weight: 700;
}

.adapter-config-card dl {
  display: grid;
  gap: var(--space-2);
  margin: var(--space-3) 0 0;
}

.adapter-config-card dl > div {
  display: grid;
  gap: 2px;
  border-bottom: 1px solid var(--border-faint);
  padding-bottom: var(--space-2);
}

.adapter-config-card dl > div:last-child {
  border-bottom: 0;
  padding-bottom: 0;
}

.adapter-config-card dt {
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-config-card dd {
  margin: 0;
  overflow-wrap: anywhere;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.45;
}

.adapter-config-card > p {
  margin: var(--space-3) 0 0;
}

.adapter-revision-list {
  display: grid;
  gap: var(--space-2);
}

.adapter-revision-card {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-revision-card.is-current {
  border-color: color-mix(in srgb, var(--accent) 32%, var(--border));
  background: color-mix(in srgb, var(--accent-soft) 42%, var(--surface));
}

.adapter-revision-card h4 + .badge {
  transform: translateY(-1px);
}

.adapter-revision-card p,
.adapter-preview-card p,
.adapter-route-preview p {
  margin: 4px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.adapter-revision-meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-revision-meta span {
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 3px 7px;
}

.adapter-test-inputs {
  display: grid;
  grid-template-columns: minmax(220px, 0.85fr) minmax(0, 1.4fr);
  gap: var(--space-3);
}

.adapter-test-inputs label {
  display: grid;
  gap: var(--space-1);
  min-width: 0;
  color: var(--text);
  font-size: 12px;
  font-weight: 700;
}

.adapter-test-inputs small {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 400;
}

.adapter-test-inputs textarea {
  margin-top: var(--space-1);
}

.adapter-test-results {
  display: grid;
  gap: var(--space-3);
  border-top: 1px solid var(--border-subtle);
  padding-top: var(--space-4);
}

.adapter-test-summary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  padding: var(--space-3);
}

.adapter-test-summary.is-verified {
  border-color: color-mix(in srgb, var(--success-fg) 30%, var(--border));
  background: var(--success-bg);
  color: var(--success-fg);
}

.adapter-test-summary.is-rejected {
  border-color: color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.adapter-test-summary strong {
  display: block;
  font-size: 13px;
}

.adapter-test-summary p {
  margin: 2px 0 0;
  color: currentColor;
  font-size: 12px;
  opacity: 0.86;
}

.adapter-test-errors,
.adapter-preview-warnings {
  display: grid;
  gap: var(--space-1);
  margin: 0;
  border-radius: var(--radius);
  padding: var(--space-3) var(--space-3) var(--space-3) 28px;
  font-size: 12px;
}

.adapter-test-errors {
  border: 1px solid color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.adapter-preview-warnings {
  margin-top: var(--space-3);
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.adapter-preview-card {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-route-preview {
  border-left: 2px solid var(--accent);
  background: var(--surface);
  padding: var(--space-3);
}

.adapter-route-preview .adapter-raw-details {
  margin-top: var(--space-3);
}

@media (max-width: 840px) {
  .adapter-hero-top {
    display: grid;
  }

  .adapter-actions {
    justify-content: flex-start;
  }

  .adapter-overview-grid,
  .adapter-config-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 600px) {
  .adapter-detail {
    padding: var(--space-2);
  }

  .adapter-hero {
    padding: var(--space-3);
  }

  .adapter-metrics,
  .adapter-poll-grid,
  .adapter-test-inputs {
    grid-template-columns: 1fr;
  }

  .adapter-endpoint {
    align-items: flex-start;
    flex-direction: column;
  }

  .adapter-tab-heading {
    align-items: flex-start;
    flex-direction: column;
  }

  .adapter-tabs {
    overflow-x: auto;
  }

  .adapter-tabs button {
    flex: 0 0 auto;
  }
}
</style>
