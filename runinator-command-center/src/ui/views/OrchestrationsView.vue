<template>
  <section class="pane flex h-full min-h-0 flex-col gap-2.5 overflow-hidden">
    <div class="panel shrink-0">
      <PanelHeader
        title="Orchestrations"
        icon="branch"
        eyebrow="Correlated work"
        description="Generic correlations, immutable execution epochs, adapters, and provider effects."
      >
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
      </PanelHeader>

      <p
        v-if="store.error"
        class="rounded-md border border-danger bg-danger-bg p-3 text-sm text-danger-fg"
      >
        {{ store.error }}
      </p>

      <div v-if="mode === 'Instances'" class="orchestration-toolbar">
        <div class="orchestration-filters">
          <label class="orchestration-filter orchestration-filter-primary"
            ><span>Find an orchestration</span>
            <div class="orchestration-search">
              <Icon name="search" :size="16" />
              <input
                v-model="filters.correlation_key"
                placeholder="Search by correlation key"
                @keyup.enter="refreshInstances"
              /></div
          ></label>
          <label class="orchestration-filter"
            ><span>Status</span
            ><select v-model="filters.status" @change="refreshInstances">
              <option value="">All statuses</option>
              <option v-for="item in statuses" :key="item" :value="item">{{ item }}</option>
            </select></label
          >
          <button class="btn btn-primary" :disabled="store.loading" @click="refreshInstances">
            <LoadingSpinner v-if="store.loading" size="sm" label="Applying filters" />
            <Icon v-else name="search" />
            <span>Search</span>
          </button>
          <button
            v-if="activeFilterCount"
            class="btn"
            :disabled="store.loading"
            @click="clearFilters"
          >
            Clear {{ activeFilterCount }}
          </button>
        </div>
        <details class="orchestration-advanced-filters">
          <summary>
            <Icon name="settings" :size="14" />
            More filters
            <span v-if="technicalFilterCount" class="adapter-count">{{
              technicalFilterCount
            }}</span>
          </summary>
          <div class="orchestration-advanced-grid">
            <label class="orchestration-filter"
              ><span>Scope</span
              ><input
                v-model="filters.scope"
                placeholder="Any scope"
                @keyup.enter="refreshInstances"
            /></label>
            <label class="orchestration-filter"
              ><span>Pipeline ID</span
              ><input
                v-model="filters.pipeline_id"
                placeholder="Exact pipeline ID"
                @keyup.enter="refreshInstances"
            /></label>
            <label class="orchestration-filter"
              ><span>Adapter ID</span
              ><input
                v-model="filters.adapter_id"
                placeholder="Exact adapter ID"
                @keyup.enter="refreshInstances"
            /></label>
          </div>
        </details>
        <span class="orchestration-result-count">
          {{ store.bindings.length }} result{{ store.bindings.length === 1 ? "" : "s" }}
        </span>
      </div>

      <div v-else class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-1">
          <p class="m-0 text-sm font-medium text-fg">Provider adapters</p>
          <HelpBubble
            text="Receive provider events, normalize them, and route them into orchestrations."
            label="About provider adapters"
          />
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
          <aside class="orchestration-list panel overflow-auto p-0">
            <div class="orchestration-list-heading">
              <div>
                <p class="adapter-eyebrow">Correlated execution</p>
                <h3>Instances</h3>
              </div>
              <span class="adapter-count">{{ store.bindings.length }}</span>
            </div>
            <div class="orchestration-list-items">
              <button
                v-for="binding in store.bindings"
                :key="binding.id"
                type="button"
                class="orchestration-list-item"
                :class="{ 'is-selected': binding.id === store.selectedId }"
                @click="openBinding(binding.id)"
              >
                <span class="orchestration-state-mark" :class="`is-${binding.status}`">
                  <Icon :name="statusIcon(binding.status)" :size="15" />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="flex items-start justify-between gap-2">
                    <span class="truncate font-medium text-fg">{{ binding.correlation_key }}</span>
                    <StatusBadge :status="binding.status" />
                  </span>
                  <span class="mt-1 block truncate text-xs text-fg-muted">{{ binding.scope }}</span>
                  <span class="mt-2 flex gap-2 text-[11px] text-fg-muted">
                    <span>Generation {{ binding.generation }}</span>
                    <span>Epoch {{ binding.current_epoch }}</span>
                    <span v-if="binding.current_phase">{{ binding.current_phase }}</span>
                  </span>
                </span>
              </button>
            </div>
            <EmptyState
              v-if="!store.loading && store.bindings.length === 0"
              compact
              icon="search"
              title="No orchestrations match these filters"
            />
          </aside>
        </template>

        <template #second>
          <div class="panel details orchestration-detail overflow-auto">
            <MobileBackBar label="Back to orchestrations" @back="showInstanceList = true" />
            <div v-if="store.detailLoading" class="grid min-h-52 place-items-center">
              <LoadingSpinner label="Loading orchestration details" />
            </div>
            <main v-else-if="store.selected" class="grid gap-4">
              <section class="orchestration-hero">
                <div class="orchestration-hero-top">
                  <div class="flex min-w-0 items-start gap-3">
                    <span class="orchestration-hero-mark" :class="`is-${store.selected.status}`">
                      <Icon :name="statusIcon(store.selected.status)" :size="20" />
                    </span>
                    <div class="min-w-0">
                      <p class="adapter-eyebrow">{{ store.selected.scope }}</p>
                      <div class="mt-1 flex flex-wrap items-center gap-2">
                        <h2 class="m-0 truncate text-xl font-semibold text-fg">
                          {{ store.selected.correlation_key }}
                        </h2>
                        <StatusBadge :status="store.selected.status" />
                      </div>
                      <p class="mt-1 mb-0 text-sm text-fg-muted">{{ orchestrationSummary }}</p>
                    </div>
                  </div>
                  <div class="orchestration-actions">
                    <button
                      v-for="(intent, name) in store.selected.policy.intents"
                      :key="name"
                      class="btn"
                      :class="
                        intent.effect === 'terminate' || intent.effect === 'supersede'
                          ? 'btn-danger'
                          : ''
                      "
                      :title="intentButtonHint(String(name))"
                      @click="openIntent(String(name))"
                    >
                      {{ name }}</button
                    ><button v-if="isSelectedTerminal" class="btn" @click="openRequeue">
                      Requeue generation
                    </button>
                  </div>
                </div>
                <div class="orchestration-metrics">
                  <MetricCard label="Generation" :value="String(store.selected.generation)" />
                  <MetricCard label="Current epoch" :value="String(store.selected.current_epoch)" />
                  <MetricCard
                    label="Phase"
                    :value="store.selected.current_phase || 'Not started'"
                  />
                  <MetricCard
                    label="Last activity"
                    :value="relativeTimestamp(store.selected.updated_at)"
                  />
                </div>
              </section>
              <nav class="orchestration-tabs" aria-label="Orchestration details" role="tablist">
                <button
                  v-for="tab in instanceTabs"
                  :key="tab"
                  type="button"
                  role="tab"
                  :aria-selected="tab === activeInstanceTab"
                  :class="{ 'is-active': tab === activeInstanceTab }"
                  @click="activeInstanceTab = tab"
                >
                  <span>{{ tab }}</span>
                  <span v-if="instanceTabCount(tab) !== null" class="orchestration-tab-count">{{
                    instanceTabCount(tab)
                  }}</span>
                </button>
              </nav>
              <section class="orchestration-workspace">
                <div v-if="activeInstanceTab === 'Timeline'" class="orchestration-tab-panel">
                  <div class="orchestration-section-heading">
                    <div>
                      <p class="adapter-eyebrow">Decision history</p>
                      <h3>Event timeline</h3>
                      <p>
                        See which signals matched, won, or were suppressed without reading the
                        reducer payload.
                      </p>
                    </div>
                  </div>
                  <div class="orchestration-timeline">
                    <article
                      v-for="event in store.events"
                      :key="event.id"
                      class="orchestration-event"
                    >
                      <span class="orchestration-event-node"
                        ><Icon :name="event.winner ? 'bolt' : 'circle'" :size="14"
                      /></span>
                      <div class="min-w-0 flex-1">
                        <div class="flex flex-wrap items-start justify-between gap-2">
                          <div>
                            <p class="adapter-eyebrow">Event {{ event.sequence }}</p>
                            <strong>{{
                              event.winner ? humanizeKey(event.winner) : "Observed event"
                            }}</strong>
                          </div>
                          <StatusBadge :status="event.disposition" />
                        </div>
                        <div class="orchestration-event-summary">
                          <span
                            ><strong>{{ event.matched_intents.length }}</strong> matched</span
                          >
                          <span
                            ><strong>{{ event.suppressed_intents.length }}</strong> suppressed</span
                          >
                          <span>{{ formatTimestamp(event.created_at) }}</span>
                        </div>
                        <div v-if="event.matched_intents.length" class="adapter-chip-list">
                          <span
                            v-for="intent in event.matched_intents"
                            :key="intent"
                            class="adapter-chip"
                            >{{ intent }}</span
                          >
                        </div>
                        <details class="adapter-raw-details">
                          <summary>View reducer details</summary>
                          <pre>{{ pretty(event.detail) }}</pre>
                        </details>
                      </div>
                    </article>
                  </div>
                  <EmptyState
                    v-if="!store.events.length"
                    compact
                    icon="clock"
                    title="No events recorded yet"
                  />
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
                <div v-else-if="activeInstanceTab === 'Resources'" class="orchestration-tab-panel">
                  <div class="orchestration-section-heading">
                    <div>
                      <p class="adapter-eyebrow">Retained context</p>
                      <h3>Resources</h3>
                      <p>Values carried forward by the current generation.</p>
                    </div>
                  </div>
                  <div v-if="resourceEntries.length" class="orchestration-data-grid">
                    <article
                      v-for="[name, value] in resourceEntries"
                      :key="name"
                      class="orchestration-data-card"
                    >
                      <span>{{ humanizeKey(name) }}</span>
                      <strong>{{ formatConfigValue(value) }}</strong>
                    </article>
                  </div>
                  <EmptyState v-else compact icon="box" title="No retained resources" />
                  <details v-if="resourceEntries.length" class="adapter-raw-details">
                    <summary>View resource JSON</summary>
                    <pre>{{ pretty(store.selected.resources) }}</pre>
                  </details>
                </div>
                <div v-else-if="activeInstanceTab === 'Budgets'" class="orchestration-tab-panel">
                  <div class="orchestration-section-heading">
                    <div>
                      <p class="adapter-eyebrow">Execution limits</p>
                      <h3>Budgets</h3>
                      <p>Attempts consumed against the policy captured for this orchestration.</p>
                    </div>
                  </div>
                  <div v-if="budgetRows.length" class="budget-list">
                    <article v-for="budget in budgetRows" :key="budget.name" class="budget-card">
                      <div class="flex items-start justify-between gap-3">
                        <div>
                          <strong>{{ humanizeKey(budget.name) }}</strong>
                          <p>
                            {{ humanizeKey(budget.exhausted) }} when exhausted<template
                              v-if="budget.handoff"
                            >
                              · hand off to {{ budget.handoff }}</template
                            >
                          </p>
                        </div>
                        <span>{{ budget.used }} / {{ budget.limit }}</span>
                      </div>
                      <div
                        class="budget-track"
                        role="progressbar"
                        :aria-valuenow="budget.used"
                        :aria-valuemax="budget.limit"
                      >
                        <span :style="{ width: `${budget.percent}%` }" />
                      </div>
                    </article>
                  </div>
                  <EmptyState v-else compact icon="percentage" title="No budgets configured" />
                </div>
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
                      ><input
                        v-model="aliasSource"
                        required
                        data-validation="identifier"
                        placeholder="github"
                    /></label>
                    <label class="grid gap-1 text-xs"
                      ><span>Scope</span
                      ><input
                        v-model="aliasScope"
                        required
                        data-validation="identifier"
                        placeholder="pull-requests"
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
                    <button class="btn btn-danger" @click="openAliasRemoval(alias)">Remove</button>
                  </article>
                  <p v-if="store.aliases.length === 0" class="text-sm text-fg-muted">
                    No alternate correlation identities route to this generation.
                  </p>
                </div>
                <div v-else-if="activeInstanceTab === 'Commands'" class="orchestration-tab-panel">
                  <div class="orchestration-section-heading">
                    <div>
                      <p class="adapter-eyebrow">Durable control plane</p>
                      <h3>Commands</h3>
                      <p>Commands issued for this orchestration and their delivery state.</p>
                    </div>
                  </div>
                  <article v-for="command in store.commands" :key="command.id" class="command-card">
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>{{ humanizeKey(command.command_type) }}</strong>
                        <p>
                          Epoch {{ command.epoch }} · {{ command.attempts }} attempt{{
                            command.attempts === 1 ? "" : "s"
                          }}
                        </p>
                      </div>
                      <StatusBadge :status="command.status" />
                    </div>
                    <code>{{ command.operation_key }}</code>
                    <details class="adapter-raw-details">
                      <summary>View command payload and result</summary>
                      <pre>{{ pretty({ payload: command.payload, result: command.result }) }}</pre>
                    </details>
                  </article>
                  <EmptyState
                    v-if="!store.commands.length"
                    compact
                    icon="list"
                    title="No commands issued"
                  />
                </div>
                <div v-else class="orchestration-tab-panel">
                  <div class="orchestration-section-heading">
                    <div>
                      <p class="adapter-eyebrow">Diagnostics</p>
                      <h3>Raw orchestration record</h3>
                      <p>The complete state for debugging or support.</p>
                    </div>
                    <button class="btn btn-sm" type="button" @click="downloadSelectedOrchestration">
                      Download JSON
                    </button>
                  </div>
                  <pre class="orchestration-raw-record">{{ pretty(store.selected) }}</pre>
                </div>
              </section>
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
                      <textarea
                        v-model="testHeaders"
                        class="min-h-28"
                        data-validation="json"
                        spellcheck="false"
                      />
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
      description="Dispatch this intent to the selected orchestration with an auditable reason and optional JSON payload."
      width="min(520px, 100%)"
      @close="intentName = null"
    >
      <form id="orchestration-intent-form" class="grid gap-3" @submit.prevent="submitIntent">
        <p
          v-if="selectedIntent"
          class="action-impact"
          :class="{ 'is-danger': intentIsDestructive }"
        >
          <strong>{{ humanizeKey(selectedIntent.effect) }}</strong>
          <span>{{ selectedIntentSummary }}</span>
        </p>
        <label>Reason<textarea v-model="reason" required class="min-h-24" /></label>
        <label
          >Payload JSON<textarea
            v-model="intentPayload"
            data-validation="json"
            class="min-h-28 font-mono text-xs"
          />
        </label>
        <p v-if="intentPayloadError" class="m-0 text-sm text-danger-fg">{{ intentPayloadError }}</p>
        <p v-if="actionError" class="m-0 text-sm text-danger-fg">{{ actionError }}</p>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="intentName = null">Cancel</button>
        <button
          class="btn"
          :class="intentIsDestructive ? 'btn-danger' : 'btn-primary'"
          type="submit"
          form="orchestration-intent-form"
          :disabled="actionPending || !reason.trim() || !!intentPayloadError"
        >
          Dispatch
        </button>
      </template>
    </Modal>
    <Modal
      v-if="requeueOpen"
      title="Requeue next generation"
      description="The next generation snapshots the current immutable pipeline and adapter revisions."
      width="min(520px, 100%)"
      @close="requeueOpen = false"
    >
      <form id="orchestration-requeue-form" class="grid gap-3" @submit.prevent="submitRequeue">
        <label>Reason<textarea v-model="reason" required class="min-h-24" /></label>
        <p v-if="actionError" class="m-0 text-sm text-danger-fg">{{ actionError }}</p>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="requeueOpen = false">Cancel</button>
        <button
          class="btn btn-primary"
          type="submit"
          form="orchestration-requeue-form"
          :disabled="actionPending || !reason.trim()"
        >
          Requeue
        </button>
      </template>
    </Modal>
    <Modal
      v-if="resolvingOperation"
      :title="`Resolve ${resolvingOperation.provider}.${resolvingOperation.action}`"
      :description="`${resolution} · ${resolvingOperation.semantics}`"
      width="min(620px, 100%)"
      @close="resolvingOperation = null"
    >
      <form
        id="orchestration-resolution-form"
        class="grid gap-3"
        @submit.prevent="submitResolution"
      >
        <label>Reason<textarea v-model="resolutionReason" required class="min-h-20" /></label>
        <label
          >Receipt JSON<textarea
            v-model="resolutionReceipt"
            data-validation="json"
            class="min-h-28 font-mono text-xs"
          />
        </label>
        <p v-if="resolutionReceiptError" class="m-0 text-sm text-danger-fg">
          {{ resolutionReceiptError }}
        </p>
        <p v-if="actionError" class="m-0 text-sm text-danger-fg">{{ actionError }}</p>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="resolvingOperation = null">Cancel</button>
        <button
          class="btn btn-primary"
          type="submit"
          form="orchestration-resolution-form"
          :disabled="actionPending || !resolutionReason.trim() || !!resolutionReceiptError"
        >
          Apply resolution
        </button>
      </template>
    </Modal>
    <Modal
      v-if="removingAlias"
      title="Remove correlation alias"
      description="Events using this alternate identity will no longer route to the selected orchestration."
      width="min(520px, 100%)"
      @close="removingAlias = null"
    >
      <div class="rounded border border-border bg-surface-subtle p-3 text-sm">
        <strong class="break-all text-fg">{{ removingAlias.correlation_key }}</strong>
        <p class="mt-1 mb-0 text-xs text-fg-muted">
          {{ removingAlias.source }} · {{ removingAlias.scope }}
        </p>
      </div>
      <p v-if="actionError" class="m-0 text-sm text-danger-fg">{{ actionError }}</p>
      <template #actions>
        <button type="button" class="btn" @click="removingAlias = null">Keep alias</button>
        <button
          type="button"
          class="btn btn-danger"
          :disabled="actionPending"
          @click="confirmAliasRemoval"
        >
          Remove alias
        </button>
      </template>
    </Modal>
    <Modal
      v-if="adapterFormOpen"
      :title="editingAdapterId ? 'Create adapter revision' : 'Set up an adapter'"
      description="Connect a provider, normalize its events, and route them into correlated orchestrations."
      width="min(980px, 100%)"
      @close="adapterFormOpen = false"
    >
      <form id="adapter-form" class="adapter-form" @submit.prevent="saveAdapter">
        <section class="adapter-form-main">
          <div class="adapter-form-section">
            <div class="adapter-form-section-heading">
              <span>1</span>
              <div>
                <h3>Choose a provider</h3>
                <p>Select the event vocabulary and verification behavior this adapter uses.</p>
              </div>
            </div>
            <div v-if="!editingAdapterId" class="adapter-kind-grid">
              <button
                v-for="kind in store.adapterKinds"
                :key="kind.kind"
                type="button"
                class="adapter-kind-option"
                :class="{ 'is-selected': adapterForm.kind === kind.kind }"
                @click="selectAdapterKind(kind.kind)"
              >
                <span class="adapter-mark">{{ adapterMark(kind.kind) }}</span>
                <span class="min-w-0 flex-1">
                  <strong>{{ kind.display_name }}</strong>
                  <small>{{ kind.description || "Provider event adapter" }}</small>
                </span>
                <Icon v-if="adapterForm.kind === kind.kind" name="check" :size="17" />
              </button>
            </div>
            <div v-else-if="formKind" class="adapter-kind-summary">
              <span class="adapter-mark">{{ adapterMark(formKind.kind) }}</span>
              <div>
                <strong>{{ formKind.display_name }}</strong>
                <p>{{ formKind.description }}</p>
              </div>
              <span class="badge status-muted">v{{ formKind.version }}</span>
            </div>
            <label class="adapter-form-field adapter-name-field">
              <span>Adapter name</span>
              <input
                v-model="adapterForm.name"
                required
                placeholder="Production GitHub events"
                autocomplete="off"
              />
              <small>Use a name operators will recognize in event history and filters.</small>
            </label>
          </div>

          <div v-if="formKind" class="adapter-form-section">
            <div class="adapter-form-section-heading">
              <span>2</span>
              <div>
                <h3>Choose delivery</h3>
                <p>Receive events immediately or let Runinator poll the provider.</p>
              </div>
            </div>
            <div class="adapter-transport-grid">
              <button
                type="button"
                class="adapter-transport-option"
                :class="{ 'is-selected': adapterForm.transport === 'webhook' }"
                :disabled="identityLocked"
                @click="selectTransport('webhook')"
              >
                <span class="adapter-section-icon"><Icon name="bolt" :size="17" /></span>
                <span
                  ><strong>Webhook</strong
                  ><small>Provider pushes events as they happen</small></span
                >
                <Icon v-if="adapterForm.transport === 'webhook'" name="check" :size="17" />
              </button>
              <button
                v-if="supportsPolling"
                type="button"
                class="adapter-transport-option"
                :class="{ 'is-selected': adapterForm.transport === 'polling' }"
                :disabled="identityLocked"
                @click="selectTransport('polling')"
              >
                <span class="adapter-section-icon"><Icon name="clock" :size="17" /></span>
                <span><strong>Polling</strong><small>Runinator checks on a schedule</small></span>
                <Icon v-if="adapterForm.transport === 'polling'" name="check" :size="17" />
              </button>
            </div>
            <p v-if="identityLocked" class="adapter-safety-note">
              <Icon name="lock" :size="15" />
              Delivery and identity are locked after the first correlation is admitted.
            </p>
          </div>

          <div v-if="formKind" class="adapter-form-section">
            <div class="adapter-form-section-heading">
              <span>3</span>
              <div>
                <h3>Configure the connection</h3>
                <p>{{ connectionStepDescription }}</p>
              </div>
            </div>
            <div v-if="adapterForm.transport === 'polling'" class="adapter-field-grid">
              <label class="adapter-form-field">
                <span>Check every</span>
                <div class="adapter-number-field">
                  <input
                    v-model.number="adapterForm.configuration.poll_interval_seconds"
                    type="number"
                    min="30"
                    max="3600"
                    required
                  />
                  <span>seconds</span>
                </div>
                <small>Between 30 seconds and one hour.</small>
              </label>
              <label
                v-if="adapterForm.kind === 'github'"
                class="adapter-form-field adapter-field-wide"
              >
                <span>Repositories</span>
                <TypedValueEditor
                  :model-value="adapterForm.configuration.repositories"
                  :ty="repositoryListType"
                  :allow-expressions="false"
                  required
                  @update:model-value="updateConfigField('repositories', $event)"
                />
                <small>One <code>owner/repository</code> per line.</small>
              </label>
              <template v-if="adapterForm.kind === 'jira'">
                <label class="adapter-form-field">
                  <span>Jira site</span>
                  <input
                    v-model="adapterForm.configuration.instance_id"
                    required
                    placeholder="acme.atlassian.net"
                  />
                  <small>A stable identity for this Jira instance.</small>
                </label>
                <label class="adapter-form-field">
                  <span>Base URL</span>
                  <input
                    v-model="adapterForm.configuration.base_url"
                    required
                    type="url"
                    placeholder="https://acme.atlassian.net"
                  />
                </label>
                <label class="adapter-form-field">
                  <span>Account email</span>
                  <input v-model="adapterForm.configuration.email" required type="email" />
                </label>
                <label class="adapter-form-field adapter-field-wide">
                  <span>Issues to watch (JQL)</span>
                  <input
                    v-model="adapterForm.configuration.jql"
                    required
                    placeholder="project = ENG AND statusCategory != Done"
                  />
                </label>
              </template>
              <label class="adapter-form-field">
                <span>{{ adapterForm.kind === "github" ? "Access token" : "API token" }}</span>
                <select
                  v-model="
                    adapterForm.secret_bindings[
                      adapterForm.kind === 'github' ? 'access_token' : 'api_token'
                    ]
                  "
                  required
                >
                  <option value="">Choose a stored secret</option>
                  <option v-for="secret in selectableSecrets" :key="secret.id" :value="secret.id">
                    {{ secret.scope }}/{{ secret.name }}
                  </option>
                </select>
                <small>The credential stays in the secret store and is never copied here.</small>
              </label>
            </div>
            <div v-else class="adapter-field-grid">
              <label
                v-for="field in configurationFields"
                :key="field.name"
                class="adapter-form-field"
              >
                <span
                  >{{ humanizeKey(field.name) }}<template v-if="field.required"> *</template></span
                >
                <TypedValueEditor
                  :model-value="adapterForm.configuration[field.name]"
                  :ty="field.value_type"
                  :allow-expressions="false"
                  :required="field.required"
                  @update:model-value="updateConfigField(field.name, $event)"
                />
                <small v-if="field.description">{{ field.description }}</small>
              </label>
              <label v-for="field in secretFields" :key="field.name" class="adapter-form-field">
                <span
                  >{{ humanizeKey(field.name) }}<template v-if="field.required"> *</template></span
                >
                <select
                  v-model="adapterForm.secret_bindings[field.name]"
                  :required="field.required"
                >
                  <option value="">Choose a stored secret</option>
                  <option v-for="secret in selectableSecrets" :key="secret.id" :value="secret.id">
                    {{ secret.scope }}/{{ secret.name }}
                  </option>
                </select>
                <small>{{
                  field.description || "Stored securely and resolved at delivery time."
                }}</small>
              </label>
            </div>
          </div>

          <details v-if="formKind" class="adapter-advanced-identity" :open="identityHasValues">
            <summary>
              <span>
                <strong>Advanced identity metadata</strong>
                <small>Optional revision identity used by custom adapter plugins.</small>
              </span>
              <span class="adapter-count">{{ identityEntryCount }}</span>
            </summary>
            <div v-if="identityLocked" class="adapter-readonly-values">
              <div v-for="[key, value] in identityEntries" :key="key">
                <span>{{ humanizeKey(key) }}</span
                ><strong>{{ formatConfigValue(value) }}</strong>
              </div>
              <p v-if="!identityEntries.length">No extra identity metadata.</p>
            </div>
            <TypedValueEditor
              v-else
              :model-value="adapterIdentity"
              :ty="identityMapType"
              :allow-expressions="false"
              @update:model-value="setAdapterIdentity"
            />
          </details>
          <p v-if="adapterFormError" class="adapter-form-error">{{ adapterFormError }}</p>
        </section>

        <aside v-if="formKind" class="adapter-form-aside">
          <div class="adapter-form-provider">
            <span class="adapter-hero-mark">{{ adapterMark(formKind.kind) }}</span>
            <div>
              <p class="adapter-eyebrow">Ready to connect</p>
              <h3>{{ formKind.display_name }}</h3>
              <p>{{ formKind.description }}</p>
            </div>
          </div>
          <div class="adapter-form-review">
            <span><Icon name="check" :size="15" /> Provider selected</span>
            <span :class="{ 'is-pending': !adapterForm.name.trim() }">
              <Icon :name="adapterForm.name.trim() ? 'check' : 'circle'" :size="15" /> Named for
              operators
            </span>
            <span
              ><Icon name="check" :size="15" />
              {{ humanizeKey(adapterForm.transport) }} delivery</span
            >
          </div>
          <div v-if="formKind.setup_instructions?.length" class="adapter-form-checklist">
            <p class="adapter-eyebrow">After saving</p>
            <ol>
              <li v-for="instruction in formKind.setup_instructions" :key="instruction">
                {{ instruction }}
              </li>
            </ol>
          </div>
          <p class="adapter-form-revision-note">
            <Icon name="lock" :size="15" />
            {{
              editingAdapterId
                ? "Saving creates a new immutable revision."
                : "Connection settings are versioned from the first save."
            }}
          </p>
        </aside>
      </form>
      <template #actions>
        <button type="button" class="btn" @click="adapterFormOpen = false">Cancel</button>
        <button
          class="btn btn-primary"
          type="submit"
          form="adapter-form"
          :disabled="adapterFormSaving || !formKind || !adapterForm.name.trim()"
        >
          <LoadingSpinner v-if="adapterFormSaving" size="sm" label="Saving adapter" />
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
  OrchestrationCorrelationAlias,
  OrchestrationEvidence,
  PipelineRunDetail,
  RuninatorType,
  WorkspaceLease,
} from "../../core/domain/models";
import type { IconName } from "../../core/domain/icons";
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
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import Modal from "../components/shared/Modal.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
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
const requeueOpen = ref(false);
const reason = ref("");
const aliasSource = ref("");
const aliasScope = ref("");
const aliasCorrelation = ref("");
const resolvingOperation = ref<ExternalOperation | null>(null);
const resolution = ref<"succeeded" | "failed" | "retry">("succeeded");
const resolutionReason = ref("");
const resolutionReceipt = ref("null");
const removingAlias = ref<OrchestrationCorrelationAlias | null>(null);
const actionPending = ref(false);
const actionError = ref<string | null>(null);
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
const adapterFormSaving = ref(false);
const adapterFormError = ref<string | null>(null);
const adapterIdentity = shallowRef<JsonValue>({});
const repositoryListType: RuninatorType = { type: "array", items: { type: "string" } };
const identityMapType: RuninatorType = { type: "map", values: { type: "any" } };
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
const secretFields = computed(() => formKind.value?.fields.filter((field) => field.secret) ?? []);
const supportsPolling = computed(() => formKind.value?.capabilities.includes("polling") ?? false);
const connectionStepDescription = computed(() =>
  adapterForm.transport === "polling"
    ? "Choose what to watch, how often to check, and which stored credential to use."
    : "Map the provider payload and choose a stored secret for delivery verification.",
);
const identityEntries = computed(() => Object.entries(jsonObject(adapterIdentity.value)));
const identityEntryCount = computed(() => identityEntries.value.length);
const identityHasValues = computed(() => identityEntryCount.value > 0);
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
const activeFilterCount = computed(
  () => Object.values(filters).filter((value) => value.trim()).length,
);
const technicalFilterCount = computed(
  () =>
    [filters.scope, filters.pipeline_id, filters.adapter_id].filter((value) => value.trim()).length,
);
const selectedIntent = computed(() =>
  intentName.value ? store.selected?.policy.intents[intentName.value] : undefined,
);
const intentIsDestructive = computed(() =>
  Boolean(selectedIntent.value && ["terminate", "supersede"].includes(selectedIntent.value.effect)),
);
const selectedIntentSummary = computed(() => {
  const intent = selectedIntent.value;

  if (!intent) {
    return "";
  }

  const configuredSignalName = intent.signal_name?.trim() ?? "";
  const signalName =
    configuredSignalName.length > 0 ? configuredSignalName : (intentName.value ?? "configured");
  const summaries = {
    terminate: "Ends the orchestration and its active execution.",
    suspend: "Pauses the active execution until a resume intent is dispatched.",
    resume: "Resumes a previously suspended execution.",
    supersede: "Stops the current execution and starts a replacement epoch.",
    observe: "Records the event without interrupting the active execution.",
    signal: `Sends the ${signalName} workflow signal.`,
  };
  return summaries[intent.effect];
});
const intentPayloadError = computed(() => jsonError(intentPayload.value, "Payload"));
const resolutionReceiptError = computed(() => jsonError(resolutionReceipt.value, "Receipt"));
const currentEpoch = computed(() =>
  store.epochs.find((epoch) => epoch.epoch === store.selected?.current_epoch),
);
const currentEpochRunId = computed(() => currentEpoch.value?.pipeline_run_id ?? null);
const currentEpochDetail = computed<PipelineRunDetail | null>(() => {
  const detail: PipelineRunDetail | null = pipelineRuns.detail;

  return detail?.run.id === currentEpochRunId.value ? detail : null;
});
const resourceEntries = computed(() => Object.entries(jsonObject(store.selected?.resources)));
const budgetRows = computed(() => {
  const selected = store.selected;

  if (!selected) {
    return [];
  }

  return Object.entries(selected.policy.budgets).map(([name, policy]) => {
    const used = selected.budgets[name] ?? 0;
    const limit = Math.max(policy.attempts, 0);

    return {
      name,
      used,
      limit,
      exhausted: policy.exhausted,
      handoff: policy.handoff,
      percent: limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0,
    };
  });
});
const orchestrationSummary = computed(() => {
  const selected = store.selected;

  if (!selected) {
    return "";
  }

  const phase = selected.current_phase ? ` in ${selected.current_phase}` : "";
  return `Generation ${String(selected.generation)}, epoch ${String(selected.current_epoch)}${phase}. Updated ${relativeTimestamp(selected.updated_at).toLowerCase()}.`;
});

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function formatTimestamp(value?: string | null): string {
  return value ? new Date(value).toLocaleString() : "Never";
}

function relativeTimestamp(value?: string | null): string {
  if (!value) {
    return "Never";
  }

  const elapsed = Date.now() - new Date(value).getTime();
  const future = elapsed < 0;
  const absolute = Math.abs(elapsed);
  const units: [number, Intl.RelativeTimeFormatUnit][] = [
    [86_400_000, "day"],
    [3_600_000, "hour"],
    [60_000, "minute"],
  ];

  for (const [milliseconds, unit] of units) {
    if (absolute >= milliseconds) {
      const amount = Math.round(absolute / milliseconds) * (future ? 1 : -1);
      return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(amount, unit);
    }
  }

  return "Just now";
}

function statusIcon(status: string): IconName {
  if (status === "completed" || status === "succeeded") {
    return "check";
  }

  if (status === "failed" || status === "terminated") {
    return "alert";
  }

  if (status === "suspended") {
    return "pause";
  }

  if (status === "waiting" || status === "pending") {
    return "clock";
  }

  return "bolt";
}

function instanceTabCount(tab: string): number | null {
  const counts: Record<string, number> = {
    Timeline: store.events.length,
    Epochs: store.epochs.length,
    Evidence: store.evidence.length,
    Operations: store.operations.length,
    Workspaces: store.workspaces.length,
    Aliases: store.aliases.length,
    Commands: store.commands.length,
  };

  return tab in counts ? counts[tab] : null;
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

function downloadSelectedOrchestration(): void {
  if (!store.selected) {
    return;
  }

  downloadJson(
    `${safeFileSegment(store.selected.correlation_key)}-orchestration.json`,
    store.selected,
  );
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

function jsonError(value: string, label: string): string | null {
  try {
    parseJson(value);
    return null;
  } catch (cause) {
    const detail = cause instanceof Error ? cause.message : "invalid JSON";
    return `${label} must be valid JSON: ${detail}`;
  }
}

function refreshInstances(): void {
  const query: Record<string, string> = {};

  for (const [key, value] of Object.entries(filters)) {
    const trimmed = value.trim();

    if (trimmed) {
      query[key] = trimmed;
    }
  }

  void store.refresh(query);
}

function clearFilters(): void {
  Object.assign(filters, {
    status: "",
    scope: "",
    correlation_key: "",
    pipeline_id: "",
    adapter_id: "",
  });
  refreshInstances();
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
  actionError.value = null;
}

function intentButtonHint(name: string): string {
  const intent = store.selected?.policy.intents[name];
  return intent ? `${humanizeKey(intent.effect)} · priority ${String(intent.priority)}` : name;
}

function openRequeue(): void {
  reason.value = "";
  actionError.value = null;
  requeueOpen.value = true;
}

async function performAction(action: () => Promise<void>): Promise<boolean> {
  actionPending.value = true;
  actionError.value = null;

  try {
    await action();
    return true;
  } catch (cause) {
    actionError.value = cause instanceof Error ? cause.message : String(cause);
    return false;
  } finally {
    actionPending.value = false;
  }
}

async function submitIntent(): Promise<void> {
  if (!intentName.value || !reason.value.trim()) {
    return;
  }

  let payload: unknown;

  try {
    payload = parseJson(intentPayload.value || "{}");
  } catch {
    return;
  }

  const intent = intentName.value;
  const saved = await performAction(() => store.dispatch(intent, reason.value.trim(), payload));

  if (saved) {
    intentName.value = null;
  }
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

  const saved = await performAction(() => store.requeue(reason.value.trim()));

  if (saved) {
    requeueOpen.value = false;
  }
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

function openAliasRemoval(alias: OrchestrationCorrelationAlias): void {
  actionError.value = null;
  removingAlias.value = alias;
}

async function confirmAliasRemoval(): Promise<void> {
  if (!removingAlias.value) {
    return;
  }

  const aliasId = removingAlias.value.id;
  const saved = await performAction(() => store.removeAlias(aliasId));

  if (saved) {
    removingAlias.value = null;
  }
}

function openResolution(operation: ExternalOperation, next: typeof resolution.value): void {
  resolvingOperation.value = operation;
  resolution.value = next;
  resolutionReason.value = "";
  resolutionReceipt.value = "null";
  actionError.value = null;
}

async function submitResolution(): Promise<void> {
  if (!resolvingOperation.value || !resolutionReason.value.trim() || resolutionReceiptError.value) {
    return;
  }

  let receipt: unknown;

  try {
    receipt = parseJson(resolutionReceipt.value || "null");
  } catch {
    return;
  }

  const operation = resolvingOperation.value;
  const saved = await performAction(() =>
    store.resolveOperation(operation, resolution.value, resolutionReason.value.trim(), receipt),
  );

  if (saved) {
    resolvingOperation.value = null;
  }
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
  adapterIdentity.value = {};

  if (adapterForm.kind !== "github" && adapterForm.kind !== "jira") {
    adapterForm.transport = "webhook";
  }

  for (const field of formKind.value?.fields ?? []) {
    if (!field.secret) {
      adapterForm.configuration[field.name] = field.default as JsonValue;
    }
  }
}

function selectAdapterKind(kind: string): void {
  if (adapterForm.kind === kind) {
    return;
  }

  adapterForm.kind = kind;
  initializeKind();
}

function selectTransport(transport: "webhook" | "polling"): void {
  if (identityLocked.value) {
    return;
  }

  adapterForm.transport = transport;

  if (transport === "polling") {
    adapterForm.configuration.poll_interval_seconds ??= 60;

    if (adapterForm.kind === "github") {
      adapterForm.configuration.repositories ??= [];
    }
  }
}

function setAdapterIdentity(value: unknown): void {
  adapterIdentity.value = (value ?? {}) as JsonValue;
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
  adapterIdentity.value = revision?.identity_configuration ?? {};
  adapterFormError.value = null;

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

  const configuration = { ...adapterForm.configuration };

  const bindings = Object.fromEntries(
    Object.entries(adapterForm.secret_bindings).filter(([, value]) => value),
  );

  adapterFormSaving.value = true;
  adapterFormError.value = null;

  try {
    await store.saveAdapter(
      {
        name: adapterForm.name.trim(),
        kind: kind.kind,
        kind_version: kind.version,
        transport: adapterForm.transport,
        configuration,
        secret_bindings: bindings,
        identity_configuration: adapterIdentity.value,
        ...(editingAdapterId.value && store.selectedAdapter
          ? { expected_revision: store.selectedAdapter.current_revision }
          : {}),
      },
      editingAdapterId.value ?? undefined,
    );
    adapterFormOpen.value = false;
  } catch (cause) {
    adapterFormError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    adapterFormSaving.value = false;
  }
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
.action-impact {
  display: grid;
  gap: 3px;
  margin: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
  color: var(--text-muted);
  font-size: 12px;
}

.action-impact strong {
  color: var(--text);
  font-size: 13px;
}

.action-impact.is-danger {
  border-color: color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.action-impact.is-danger strong {
  color: var(--danger-fg);
}

.orchestration-toolbar {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: end;
  gap: var(--space-2) var(--space-3);
}

.orchestration-filters {
  display: flex;
  align-items: end;
  gap: var(--space-2);
  min-width: 0;
}

.orchestration-filter {
  display: grid;
  gap: 4px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 600;
}

.orchestration-filter-primary {
  width: min(420px, 44vw);
}

.orchestration-filter select {
  min-width: 145px;
}

.orchestration-search {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding-left: 10px;
  color: var(--text-muted);
}

.orchestration-search:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.orchestration-search input {
  min-width: 0;
  flex: 1;
  border: 0;
  background: transparent;
  padding-left: 0;
  box-shadow: none;
}

.orchestration-search input:focus {
  box-shadow: none;
}

.orchestration-advanced-filters {
  position: relative;
}

.orchestration-advanced-filters > summary {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 36px;
  cursor: pointer;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 0 10px;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 600;
}

.orchestration-advanced-filters > summary::-webkit-details-marker {
  display: none;
}

.orchestration-advanced-grid {
  position: absolute;
  z-index: 15;
  top: calc(100% + 6px);
  right: 0;
  display: grid;
  width: min(560px, 86vw);
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-raised, var(--surface));
  box-shadow: var(--shadow-lg);
  padding: var(--space-4);
}

.orchestration-advanced-grid label:first-child {
  grid-column: 1 / -1;
}

.orchestration-result-count {
  align-self: center;
  color: var(--text-muted);
  font-size: 11px;
  white-space: nowrap;
}

.orchestration-list {
  gap: 0;
  background: var(--surface);
}

.orchestration-list-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  padding: var(--space-4);
}

.orchestration-list-heading h3 {
  margin: 3px 0 0;
  color: var(--text);
  font-size: 14px;
}

.orchestration-list-items {
  display: grid;
  gap: 3px;
  padding: var(--space-2);
}

.orchestration-list-item {
  display: flex;
  width: 100%;
  align-items: flex-start;
  gap: var(--space-3);
  border: 1px solid transparent;
  border-radius: var(--radius);
  background: transparent;
  padding: 10px;
  color: inherit;
  text-align: left;
  transition:
    background-color 150ms ease,
    border-color 150ms ease,
    transform 150ms ease;
}

.orchestration-list-item:hover {
  border-color: var(--border-subtle);
  background: var(--surface-hover);
}

.orchestration-list-item:active {
  transform: scale(0.99);
}

.orchestration-list-item.is-selected {
  border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  background: var(--accent-soft);
  box-shadow: inset 3px 0 0 var(--accent);
}

.orchestration-state-mark,
.orchestration-hero-mark {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  border: 1px solid color-mix(in srgb, var(--accent) 24%, var(--border));
  border-radius: var(--radius);
  background: var(--accent-soft);
  color: var(--accent-text);
}

.orchestration-state-mark {
  width: 30px;
  height: 30px;
}

.orchestration-state-mark.is-completed,
.orchestration-hero-mark.is-completed {
  border-color: color-mix(in srgb, var(--success-fg) 30%, var(--border));
  background: var(--success-bg);
  color: var(--success-fg);
}

.orchestration-state-mark.is-failed,
.orchestration-state-mark.is-terminated,
.orchestration-hero-mark.is-failed,
.orchestration-hero-mark.is-terminated {
  border-color: color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.orchestration-state-mark.is-waiting,
.orchestration-state-mark.is-suspended,
.orchestration-hero-mark.is-waiting,
.orchestration-hero-mark.is-suspended {
  border-color: color-mix(in srgb, var(--warning-fg) 30%, var(--border));
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.orchestration-detail {
  gap: var(--space-4);
  padding: var(--space-4);
}

.orchestration-hero {
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

.orchestration-hero-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.orchestration-hero-mark {
  width: 42px;
  height: 42px;
}

.orchestration-actions {
  display: flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.orchestration-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--space-2);
}

.orchestration-tabs {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--surface-sunken);
  padding: 0 var(--space-2);
}

.orchestration-tabs button {
  position: relative;
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: 0;
  background: transparent;
  padding: 10px var(--space-3);
  color: var(--text-muted);
  font-size: 12px;
}

.orchestration-tabs button:hover {
  background: transparent;
  color: var(--text);
}

.orchestration-tabs button::after {
  position: absolute;
  right: var(--space-3);
  bottom: -1px;
  left: var(--space-3);
  height: 2px;
  content: "";
  background: transparent;
}

.orchestration-tabs button.is-active {
  color: var(--accent-text);
  font-weight: 700;
}

.orchestration-tabs button.is-active::after {
  background: var(--accent);
}

.orchestration-tab-count {
  min-width: 18px;
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 10px;
  text-align: center;
}

.orchestration-workspace {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  background: var(--surface);
}

.orchestration-tab-panel {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-4);
}

.orchestration-section-heading {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-3);
}

.orchestration-section-heading h3 {
  margin: 3px 0 0;
  color: var(--text);
  font-size: 15px;
}

.orchestration-section-heading p:not(.adapter-eyebrow) {
  margin: 3px 0 0;
  color: var(--text-muted);
  font-size: 12px;
}

.orchestration-timeline {
  display: grid;
}

.orchestration-event {
  position: relative;
  display: flex;
  gap: var(--space-3);
  padding: 0 0 var(--space-4);
}

.orchestration-event:not(:last-child)::before {
  position: absolute;
  top: 30px;
  bottom: 0;
  left: 15px;
  width: 1px;
  content: "";
  background: var(--border-subtle);
}

.orchestration-event-node {
  z-index: 1;
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 31px;
  height: 31px;
  border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border));
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent-text);
}

.orchestration-event > div {
  display: grid;
  gap: var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.orchestration-event-summary {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  color: var(--text-muted);
  font-size: 11px;
}

.orchestration-event-summary strong {
  color: var(--text);
}

.orchestration-data-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2);
}

.orchestration-data-card,
.command-card,
.budget-card {
  min-width: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.orchestration-data-card {
  display: grid;
  gap: 4px;
}

.orchestration-data-card span {
  color: var(--text-muted);
  font-size: 11px;
}

.orchestration-data-card strong {
  overflow-wrap: anywhere;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 12px;
}

.budget-list,
.command-card {
  display: grid;
  gap: var(--space-2);
}

.budget-card p,
.command-card p {
  margin: 3px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}

.budget-card > div > span {
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 700;
}

.budget-track {
  height: 6px;
  overflow: hidden;
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
}

.budget-track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
}

.command-card code {
  overflow-wrap: anywhere;
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: 11px;
}

.orchestration-raw-record {
  max-height: min(560px, 60vh);
  margin: 0;
  overflow: auto;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-sunken);
  padding: var(--space-3);
  color: var(--text-subtle);
  font-size: 11px;
}

.adapter-form {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 250px;
  gap: var(--space-5);
}

.adapter-form-main,
.adapter-form-section {
  display: grid;
  gap: var(--space-4);
}

.adapter-form-main {
  min-width: 0;
}

.adapter-form-section {
  border-bottom: 1px solid var(--border-subtle);
  padding-bottom: var(--space-5);
}

.adapter-form-section-heading {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}

.adapter-form-section-heading > span {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 25px;
  height: 25px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent-text);
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 800;
}

.adapter-form-section-heading h3,
.adapter-form-provider h3 {
  margin: 0;
  color: var(--text);
  font-size: 14px;
}

.adapter-form-section-heading p,
.adapter-form-provider p {
  margin: 3px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.adapter-kind-grid,
.adapter-transport-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2);
}

.adapter-kind-option,
.adapter-transport-option {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: var(--space-3);
  color: var(--text-muted);
  text-align: left;
}

.adapter-kind-option:hover,
.adapter-transport-option:hover {
  border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  background: var(--surface-hover);
}

.adapter-kind-option.is-selected,
.adapter-transport-option.is-selected {
  border-color: var(--accent);
  background: var(--accent-soft);
  color: var(--accent-text);
  box-shadow: inset 0 0 0 1px var(--accent);
}

.adapter-kind-option strong,
.adapter-transport-option strong {
  display: block;
  color: var(--text);
  font-size: 12px;
}

.adapter-kind-option small,
.adapter-transport-option small {
  display: block;
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.35;
}

.adapter-kind-summary {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-kind-summary > div {
  min-width: 0;
  flex: 1;
}

.adapter-kind-summary p {
  margin: 2px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-form-field {
  display: grid;
  align-content: start;
  gap: 5px;
  min-width: 0;
  color: var(--text);
  font-size: 12px;
  font-weight: 700;
}

.adapter-form-field > small {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 400;
  line-height: 1.4;
}

.adapter-name-field {
  max-width: 460px;
}

.adapter-field-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
}

.adapter-field-wide {
  grid-column: 1 / -1;
}

.adapter-number-field {
  display: flex;
  align-items: stretch;
}

.adapter-number-field input {
  min-width: 0;
  border-radius: var(--radius) 0 0 var(--radius);
}

.adapter-number-field span {
  display: inline-flex;
  align-items: center;
  border: 1px solid var(--border);
  border-left: 0;
  border-radius: 0 var(--radius) var(--radius) 0;
  background: var(--surface-subtle);
  padding: 0 10px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
}

.adapter-advanced-identity {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.adapter-advanced-identity > summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  cursor: pointer;
  list-style: none;
  padding: var(--space-3);
}

.adapter-advanced-identity > summary::-webkit-details-marker {
  display: none;
}

.adapter-advanced-identity > summary strong,
.adapter-advanced-identity > summary small {
  display: block;
}

.adapter-advanced-identity > summary strong {
  color: var(--text);
  font-size: 12px;
}

.adapter-advanced-identity > summary small {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-advanced-identity > .typed-value-editor,
.adapter-readonly-values {
  border-top: 1px solid var(--border-subtle);
  padding: var(--space-3);
}

.adapter-readonly-values {
  display: grid;
  gap: var(--space-2);
}

.adapter-readonly-values > div {
  display: flex;
  justify-content: space-between;
  gap: var(--space-3);
  color: var(--text-muted);
  font-size: 11px;
}

.adapter-readonly-values strong {
  overflow-wrap: anywhere;
  color: var(--text);
  font-family: var(--font-mono);
}

.adapter-form-aside {
  display: grid;
  align-content: start;
  gap: var(--space-4);
  border-left: 1px solid var(--border-subtle);
  padding-left: var(--space-5);
}

.adapter-form-provider {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}

.adapter-form-review {
  display: grid;
  gap: var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.adapter-form-review span {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--success-fg);
  font-size: 11px;
}

.adapter-form-review span.is-pending {
  color: var(--text-muted);
}

.adapter-form-checklist ol {
  display: grid;
  gap: var(--space-2);
  margin: var(--space-2) 0 0;
  padding-left: 18px;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.45;
}

.adapter-form-revision-note,
.adapter-form-error {
  margin: 0;
  border-radius: var(--radius);
  padding: var(--space-3);
  font-size: 11px;
  line-height: 1.45;
}

.adapter-form-revision-note {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  background: var(--accent-soft);
  color: var(--accent-text);
}

.adapter-form-error {
  border: 1px solid color-mix(in srgb, var(--danger-fg) 30%, var(--border));
  background: var(--danger-bg);
  color: var(--danger-fg);
}

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
  .orchestration-toolbar {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .orchestration-result-count {
    display: none;
  }

  .orchestration-hero-top {
    display: grid;
  }

  .orchestration-actions {
    justify-content: flex-start;
  }

  .orchestration-metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .adapter-form {
    grid-template-columns: 1fr;
  }

  .adapter-form-aside {
    border-top: 1px solid var(--border-subtle);
    border-left: 0;
    padding-top: var(--space-4);
    padding-left: 0;
  }

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
  .orchestration-toolbar {
    grid-template-columns: 1fr auto;
  }

  .orchestration-filters {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .orchestration-filter-primary {
    width: auto;
    grid-column: 1 / -1;
  }

  .orchestration-advanced-grid {
    position: fixed;
    top: auto;
    right: var(--space-3);
    bottom: var(--space-3);
    left: var(--space-3);
    width: auto;
    grid-template-columns: 1fr;
  }

  .orchestration-advanced-grid label:first-child {
    grid-column: auto;
  }

  .orchestration-detail {
    padding: var(--space-2);
  }

  .orchestration-hero {
    padding: var(--space-3);
  }

  .orchestration-metrics,
  .orchestration-data-grid,
  .adapter-kind-grid,
  .adapter-transport-grid,
  .adapter-field-grid {
    grid-template-columns: 1fr;
  }

  .adapter-field-wide {
    grid-column: auto;
  }

  .orchestration-section-heading {
    align-items: flex-start;
    flex-direction: column;
  }

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
