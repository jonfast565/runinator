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

      <div v-else class="flex flex-wrap items-center gap-2">
        <button class="btn btn-primary" @click="openAdapterForm()">
          <Icon name="plus" />
          <span>New adapter</span>
        </button>
        <button class="btn" :disabled="store.loading" @click="refreshAdapters">
          <LoadingSpinner v-if="store.loading" size="sm" label="Refreshing adapters" />
          <Icon v-else name="refresh" />
          <span>Refresh</span>
        </button>
        <button class="btn" @click="checkHost">Host health</button>
        <button class="btn" @click="reloadHost">Reload plugins</button>
        <pre v-if="hostResult" class="output max-w-xl">{{ pretty(hostResult) }}</pre>
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
          <aside class="panel overflow-auto p-0">
            <div class="border-b border-border p-3">
              <h3 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">
                Adapter kinds
              </h3>
              <div class="mt-2 grid gap-1">
                <div
                  v-for="entry in adapterCatalog"
                  :key="`${entry.metadata.kind}:${entry.origin}`"
                  class="rounded bg-surface-subtle px-2 py-1 text-xs"
                  :title="entry.error || entry.metadata.description || ''"
                >
                  <div class="flex items-center justify-between gap-2">
                    <span>{{ entry.metadata.display_name }} v{{ entry.metadata.version }}</span
                    ><span :class="entry.healthy ? 'text-success-fg' : 'text-danger-fg'">{{
                      entry.healthy ? "healthy" : "error"
                    }}</span>
                  </div>
                  <p class="truncate text-fg-muted">{{ entry.origin }}</p>
                  <p v-if="entry.error" class="mt-1 text-danger-fg">{{ entry.error }}</p>
                </div>
              </div>
            </div>
            <button
              v-for="adapter in store.adapters"
              :key="adapter.id"
              class="block w-full border-b border-border p-3 text-left hover:bg-surface-hover"
              :class="{ 'bg-surface-muted': adapter.id === store.selectedAdapterId }"
              @click="openAdapter(adapter.id)"
            >
              <div class="flex justify-between gap-2">
                <span class="truncate font-medium">{{ adapter.name }}</span
                ><span
                  class="text-xs"
                  :class="adapter.enabled ? 'text-success-fg' : 'text-fg-muted'"
                  >{{ adapter.enabled ? "enabled" : "disabled" }}</span
                >
              </div>
              <p class="mt-1 text-xs text-fg-muted">
                {{ adapter.kind }} · revision {{ adapter.current_revision }}
              </p>
            </button>
            <EmptyState
              v-if="store.adapters.length === 0"
              compact
              icon="box"
              title="No adapters configured"
            />
          </aside>
        </template>

        <template #second>
          <div class="panel details overflow-auto">
            <MobileBackBar label="Back to adapters" @back="showAdapterList = true" />
            <main v-if="store.selectedAdapter">
              <div class="flex flex-wrap justify-between gap-3">
                <div>
                  <h2 class="text-lg font-semibold">{{ store.selectedAdapter.name }}</h2>
                  <p class="text-sm text-fg-muted">
                    {{ selectedKind?.display_name || store.selectedAdapter.kind }} ·
                    {{ currentTransport }} · immutable revision
                    {{ store.selectedAdapter.current_revision }}
                  </p>
                </div>
                <div class="flex flex-wrap gap-2">
                  <button v-if="currentTransport === 'webhook'" class="btn" @click="copyWebhook">
                    Copy webhook URL</button
                  ><button class="btn" @click="openAdapterForm(store.selectedAdapter)">Edit</button
                  ><button class="btn" @click="openAdapterForm(store.selectedAdapter, true)">
                    Clone</button
                  ><button class="btn" @click="toggleSelectedAdapter">
                    {{ store.selectedAdapter.enabled ? "Disable" : "Enable" }}</button
                  ><button
                    class="btn"
                    :disabled="store.selectedAdapter.has_admitted_binding"
                    @click="removeSelectedAdapter"
                  >
                    Delete
                  </button>
                </div>
              </div>
              <code
                v-if="currentTransport === 'webhook'"
                class="mt-3 block break-all rounded bg-surface-subtle p-2 text-xs"
                >{{ webhookPath }}</code
              >
              <section
                v-else-if="store.adapterPollStatus"
                class="mt-3 grid gap-3 rounded border border-border bg-surface-subtle p-3 text-sm md:grid-cols-3"
              >
                <div>
                  <strong>Next poll</strong>
                  <p class="text-fg-muted">
                    {{ formatTimestamp(store.adapterPollStatus.next_poll_at) }}
                  </p>
                </div>
                <div>
                  <strong>Last success</strong>
                  <p class="text-fg-muted">
                    {{ formatTimestamp(store.adapterPollStatus.last_success_at) }}
                  </p>
                </div>
                <div>
                  <strong>Last attempt</strong>
                  <p class="text-fg-muted">
                    {{ formatTimestamp(store.adapterPollStatus.last_attempt_at) }}
                  </p>
                </div>
                <div v-if="store.adapterPollStatus.last_error" class="md:col-span-3">
                  <strong class="text-danger-fg">Last error</strong>
                  <p class="mt-1 text-danger-fg">{{ store.adapterPollStatus.last_error }}</p>
                </div>
                <details class="md:col-span-3">
                  <summary class="cursor-pointer text-xs text-fg-muted">Durable checkpoint</summary>
                  <pre class="mt-2 overflow-auto text-xs">{{
                    pretty(store.adapterPollStatus.checkpoint)
                  }}</pre>
                </details>
              </section>
              <p
                v-if="store.selectedAdapter.has_admitted_binding"
                class="mt-2 text-xs text-fg-muted"
              >
                Identity extraction and transport are locked because this adapter has admitted a
                correlation.
              </p>
              <div v-if="selectedKind" class="mt-3 grid gap-2 text-xs md:grid-cols-3">
                <div>
                  <strong>Capabilities</strong>
                  <p class="text-fg-muted">
                    {{ selectedKind.capabilities.join(", ") || "normalize" }}
                  </p>
                </div>
                <div>
                  <strong>Canonical events</strong>
                  <p class="text-fg-muted">
                    {{ selectedKind.event_names.join(", ") || "provider-defined" }}
                  </p>
                </div>
                <div>
                  <strong>Canonical pointers</strong>
                  <p class="break-all text-fg-muted">
                    {{ selectedKind.canonical_pointers.join(", ") || "provider-defined" }}
                  </p>
                </div>
              </div>
              <section
                v-if="selectedKind?.setup_instructions?.length"
                class="mt-3 rounded border border-border bg-surface-subtle p-3 text-sm"
              >
                <strong>Provider setup</strong>
                <ol class="mt-2 list-decimal space-y-1 pl-5 text-fg-muted">
                  <li v-for="instruction in selectedKind.setup_instructions" :key="instruction">
                    {{ instruction }}
                  </li>
                </ol>
              </section>
              <nav class="mt-5 flex gap-1 border-b border-border">
                <button
                  v-for="tab in adapterTabs"
                  :key="tab"
                  class="px-3 py-2 text-sm"
                  :class="tab === activeAdapterTab ? 'border-b-2 border-accent' : 'text-fg-muted'"
                  @click="activeAdapterTab = tab"
                >
                  {{ tab }}
                </button>
              </nav>
              <pre v-if="activeAdapterTab === 'Configuration'" class="mt-4 overflow-auto text-xs">{{
                pretty(currentAdapterRevision)
              }}</pre>
              <pre
                v-else-if="activeAdapterTab === 'Revisions'"
                class="mt-4 overflow-auto text-xs"
                >{{ pretty(store.adapterRevisions) }}</pre>
              <div v-else class="mt-4 grid gap-3">
                <label class="text-sm"
                  >Headers JSON<textarea
                    v-model="testHeaders"
                    class="mt-1 min-h-24 w-full font-mono text-xs"
                  />
                </label>
                <label class="text-sm"
                  >Sample request body<textarea
                    v-model="testBody"
                    class="mt-1 min-h-40 w-full font-mono text-xs"
                  />
                </label>
                <button class="btn w-fit" @click="runTest">
                  Verify, normalize, and preview routes
                </button>
                <section v-if="testResult" class="grid gap-3">
                  <div class="flex items-center gap-2 rounded border border-border p-3 text-sm">
                    <span
                      class="rounded px-2 py-1 text-xs"
                      :class="
                        testResult.verified
                          ? 'bg-success-bg text-success-fg'
                          : 'bg-danger-bg text-danger-fg'
                      "
                      >{{ testResult.verified ? "Verified" : "Rejected" }}</span
                    >
                    <span class="text-fg-muted"
                      >{{ testResult.events.length }} normalized event(s)</span
                    >
                  </div>
                  <ul
                    v-if="testResult.errors.length"
                    class="rounded border border-danger bg-danger-bg p-3 text-sm text-danger-fg"
                  >
                    <li v-for="error in testResult.errors" :key="error">{{ error }}</li>
                  </ul>
                  <article
                    v-for="preview in testResult.previews"
                    :key="preview.delivery_id"
                    class="rounded border border-border p-3"
                  >
                    <div class="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <strong>{{ preview.event_type }}</strong>
                        <p class="text-xs text-fg-muted">
                          {{ preview.scope }}/{{ preview.correlation_key }}
                        </p>
                      </div>
                      <span class="rounded bg-surface-subtle px-2 py-1 text-xs">{{
                        preview.lifecycle
                      }}</span>
                    </div>
                    <ul
                      v-if="preview.validation_errors.length"
                      class="mt-3 rounded bg-warning-bg p-2 text-xs text-warning-fg"
                    >
                      <li v-for="error in preview.validation_errors" :key="error">{{ error }}</li>
                    </ul>
                    <div
                      v-for="match in preview.pipeline_matches"
                      :key="match.pipeline_id"
                      class="mt-3 rounded bg-surface-subtle p-3 text-sm"
                    >
                      <div class="flex flex-wrap justify-between gap-2">
                        <strong>{{ match.pipeline_name }}</strong
                        ><span class="text-xs text-fg-muted">{{
                          match.managed ? "managed" : "unmanaged"
                        }}</span>
                      </div>
                      <p class="mt-2 text-xs text-fg-muted">
                        Matched actions: {{ match.routes.map((route) => route.action).join(", ") }}
                      </p>
                      <p class="mt-1 text-xs">
                        Candidate intents: {{ match.candidate_intents.join(", ") || "none" }}
                      </p>
                      <p v-if="match.winner" class="mt-1 text-xs">
                        <strong>Winner:</strong> {{ match.winner
                        }}<template v-if="match.suppressed_intents.length">
                          · suppressed {{ match.suppressed_intents.join(", ") }}</template
                        >
                      </p>
                      <details class="mt-2">
                        <summary class="cursor-pointer text-xs text-fg-muted">
                          Matched route details
                        </summary>
                        <pre class="mt-2 overflow-auto text-xs">{{ pretty(match.routes) }}</pre>
                      </details>
                    </div>
                  </article>
                  <details>
                    <summary class="cursor-pointer text-xs text-fg-muted">
                      Raw normalized response
                    </summary>
                    <pre class="mt-2 overflow-auto rounded bg-surface-subtle p-3 text-xs">{{
                      pretty(testResult)
                    }}</pre>
                  </details>
                </section>
              </div>
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
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import Modal from "../components/shared/Modal.vue";
import SplitPane from "../components/shared/SplitPane.vue";
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
