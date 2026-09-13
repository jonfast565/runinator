<template>
  <div class="orchestration-editor">
    <section class="orchestration-status" :class="enabled ? 'is-enabled' : ''">
      <div class="flex min-w-0 items-start gap-3">
        <span class="orchestration-status-icon">
          <Icon :name="enabled ? 'check' : 'branch'" :size="17" />
        </span>
        <div class="min-w-0">
          <p class="orchestration-eyebrow">
            {{ enabled ? "Orchestration enabled" : "Orchestration disabled" }}
          </p>
          <h3>Manage correlated executions</h3>
          <p v-if="enabled">
            One durable execution per correlation key, controlled by incoming events.
          </p>
          <p v-else>
            Provider events will not create or control correlated runs for this pipeline.
          </p>
          <div v-if="enabled" class="orchestration-status-metrics" aria-label="Policy summary">
            <span
              ><strong>{{ routes.length }}</strong> routes</span
            >
            <span
              ><strong>{{ intents.length }}</strong> intents</span
            >
            <span
              ><strong>{{ budgets.length }}</strong> retry budgets</span
            >
            <span
              ><strong>{{ workspaceCount }}</strong> workspaces</span
            >
          </div>
        </div>
      </div>
      <button
        v-if="enabled"
        type="button"
        class="btn btn-danger btn-sm"
        @click="disableConfirmOpen = true"
      >
        Disable…
      </button>
    </section>

    <section
      v-if="!enabled"
      class="grid gap-4 rounded border border-border bg-surface-subtle p-4 text-sm"
    >
      <div class="flex items-center gap-1">
        <h3 class="m-0 text-base font-semibold text-fg">Set up correlated execution</h3>
        <HelpBubble
          text="Connect external events to this pipeline by matching provider events to a correlation scope and lifecycle. Define whether updates observe, pause, restart, or signal a run, then map the result, evidence, and workspace state retained for each phase."
          label="About correlated execution"
        />
      </div>
      <div>
        <button type="button" class="btn btn-primary" @click="enableConfiguration">
          Enable orchestration
        </button>
      </div>
    </section>

    <template v-else>
      <section v-if="issues.length" class="orchestration-errors" aria-live="polite">
        <div>
          <strong>{{ issues.length }} issue{{ issues.length === 1 ? "" : "s" }} to fix</strong>
          <p>Open a highlighted section to review its fields.</p>
        </div>
        <div class="flex flex-wrap gap-1.5">
          <button
            v-for="item in issueTabs"
            :key="item.tab"
            type="button"
            class="btn btn-sm"
            @click="tab = item.tab"
          >
            {{ tabTitle(item.tab) }} · {{ item.count }}
          </button>
        </div>
      </section>

      <div class="orchestration-workbench">
        <nav class="orchestration-steps" aria-label="Orchestration settings" role="tablist">
          <button
            v-for="(item, index) in tabs"
            :id="tabDomId(item)"
            :key="item"
            type="button"
            role="tab"
            :aria-controls="`${tabDomId(item)}-panel`"
            :aria-selected="tab === item"
            :class="{ 'is-active': tab === item, 'has-errors': tabIssueCount(item) > 0 }"
            @click="tab = item"
          >
            <span class="orchestration-step-number">{{ index + 1 }}</span>
            <span class="orchestration-step-copy">
              <strong>{{ tabTitle(item) }}</strong>
              <small>{{ tabSummary(item) }}</small>
            </span>
            <span v-if="tabIssueCount(item)" class="orchestration-tab-count">
              {{ tabIssueCount(item) }}
            </span>
            <Icon v-else-if="tabComplete(item)" name="check" :size="14" />
          </button>
        </nav>

        <section
          :id="`${tabDomId(tab)}-panel`"
          class="orchestration-stage"
          role="tabpanel"
          :aria-labelledby="tabDomId(tab)"
        >
          <header class="orchestration-stage-header">
            <p class="orchestration-eyebrow">
              Step {{ tabs.indexOf(tab) + 1 }} of {{ tabs.length }}
            </p>
            <h2>{{ tabTitle(tab) }}</h2>
            <p>{{ tabDescription(tab) }}</p>
          </header>

          <section
            v-if="activeTabIssues.length"
            class="orchestration-inline-errors"
            aria-live="polite"
          >
            <strong>Needs attention</strong>
            <ul>
              <li v-for="issue in activeTabIssues" :key="issue.message">{{ issue.message }}</li>
            </ul>
          </section>

          <div v-if="tab === 'Admission Routes'" class="grid gap-4">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Event admission</h3>
                  <HelpBubble label="About admission routes">
                    <strong>Incoming events</strong>
                    <p>
                      Routes decide which provider events start, record, or control correlated work.
                      They are checked in order against the current lifecycle.
                    </p>
                  </HelpBubble>
                </div>
              </div>
              <button type="button" class="btn btn-primary btn-sm" @click="addRoute">
                <Icon name="plus" :size="15" />
                Add route
              </button>
            </header>
            <section class="orchestration-scope-card">
              <div>
                <strong>Correlation scope</strong>
                <p>Every unique correlation key within this scope controls one execution.</p>
              </div>
              <label class="orchestration-field">
                <span>Scope name</span>
                <input v-model="scope" required placeholder="ticket.lifecycle" />
              </label>
            </section>
            <section
              v-if="routes.length === 0"
              class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
            >
              <strong class="block text-fg">No admission routes yet</strong>
              Add a provider event that should start, record, or control a correlated pipeline run.
            </section>
            <article
              v-for="(route, routeIndex) in routes"
              :key="route.id"
              class="orchestration-card route-card"
            >
              <div class="orchestration-card-heading">
                <div>
                  <p class="orchestration-eyebrow">Rule {{ routeIndex + 1 }}</p>
                  <strong>{{ routeSummary(route) }}</strong>
                </div>
                <button
                  type="button"
                  class="btn btn-ghost btn-sm text-danger-fg"
                  @click="routes.splice(routeIndex, 1)"
                >
                  Remove route
                </button>
              </div>
              <div class="route-builder">
                <label class="orchestration-field route-event"
                  ><span>When this event arrives</span
                  ><input v-model="route.event_type" required placeholder="issue.updated"
                /></label>
                <label class="orchestration-field"
                  ><span>And the execution is</span
                  ><select v-model="route.lifecycle" @change="normalizeRoute(route)">
                    <option value="unbound">Not started</option>
                    <option value="active">Running</option>
                    <option value="terminal">Finished</option>
                  </select></label
                >
                <label class="orchestration-field"
                  ><span>Then</span
                  ><select v-model="route.action" @change="normalizeRoute(route)">
                    <option
                      v-for="action in actionsFor(route.lifecycle)"
                      :key="action"
                      :value="action"
                    >
                      {{ actionLabel(action) }}
                    </option>
                  </select>
                  <small>{{ routeActionHint(route) }}</small></label
                >
                <label v-if="route.action === 'dispatch'" class="orchestration-field"
                  ><span>Using response</span
                  ><select v-model="route.intent" :disabled="route.action !== 'dispatch'">
                    <option value="">Choose a response…</option>
                    <option v-for="intent in intents" :key="intent.id" :value="intent.name">
                      {{ intent.name }}
                    </option>
                  </select></label
                >
              </div>
              <details class="orchestration-details" :open="route.predicates.length > 0">
                <summary>
                  <span>Conditions</span>
                  <small>{{
                    route.predicates.length
                      ? `${route.predicates.length} configured`
                      : "Always match this event"
                  }}</small>
                </summary>
                <div class="grid gap-2">
                  <div
                    v-for="(predicate, predicateIndex) in route.predicates"
                    :key="predicate.id"
                    class="predicate-row"
                  >
                    <span class="predicate-prefix">Only if</span>
                    <input
                      v-model="predicate.pointer"
                      list="orchestration-pointers"
                      placeholder="/payload/path"
                      aria-label="Payload field"
                    />
                    <select v-model="predicate.operator" aria-label="Comparison">
                      <option value="equal">equals</option>
                      <option value="not_equal">does not equal</option>
                      <option value="in">is in</option>
                      <option value="contains">contains</option>
                      <option value="exists">exists</option>
                    </select>
                    <input
                      v-if="predicate.operator !== 'exists'"
                      v-model="predicate.valueText"
                      placeholder='JSON value, e.g. "ready"'
                      aria-label="Comparison value"
                    />
                    <button
                      type="button"
                      class="btn btn-ghost btn-sm"
                      aria-label="Remove condition"
                      @click="route.predicates.splice(predicateIndex, 1)"
                    >
                      <Icon name="close" :size="14" />
                    </button>
                  </div>
                  <button type="button" class="btn btn-sm w-fit" @click="addPredicate(route)">
                    <Icon name="plus" :size="14" /> Add condition
                  </button>
                </div>
              </details>
            </article>
          </div>

          <div v-else-if="tab === 'Intents'" class="grid gap-4">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Intents</h3>
                  <HelpBubble label="About named responses">
                    <strong>Responses</strong>
                    <p>
                      Intents describe how active work responds to an event. Priority decides which
                      response wins when several match.
                    </p>
                  </HelpBubble>
                </div>
              </div>
              <button type="button" class="btn btn-primary btn-sm" @click="addIntent">
                <Icon name="plus" :size="15" />
                Add intent
              </button>
            </header>
            <section
              v-if="intents.length === 0"
              class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
            >
              <strong class="block text-fg">No intents yet</strong>
              Add an intent before an admission route can pause, restart, or signal an active run.
            </section>
            <article
              v-for="(intent, index) in intents"
              :key="intent.id"
              class="orchestration-card intent-card"
            >
              <div class="orchestration-card-heading">
                <div>
                  <p class="orchestration-eyebrow">Intent {{ index + 1 }}</p>
                  <strong>{{ intent.name || "Unnamed intent" }}</strong>
                </div>
                <button
                  type="button"
                  class="btn btn-ghost btn-sm text-danger-fg"
                  :disabled="intentReferenceCount(intent.name) > 0"
                  :title="intentRemovalHint(intent.name)"
                  @click="removeIntent(index)"
                >
                  Remove intent
                </button>
              </div>
              <div class="intent-primary-grid">
                <label class="orchestration-field"
                  ><span>Response name</span><input v-model="intent.name" placeholder="refresh"
                /></label>
                <label class="orchestration-field">
                  <span>What should happen?</span>
                  <select v-model="intent.effect">
                    <option v-for="effect in effects" :key="effect" :value="effect">
                      {{ effectLabel(effect) }}
                    </option>
                  </select>
                  <small>{{ effectDescription(intent.effect) }}</small>
                </label>
                <label class="orchestration-field priority-field">
                  <span>Priority</span>
                  <input v-model.number="intent.priority" type="number" />
                  <small>Higher wins when responses arrive together.</small>
                </label>
              </div>
              <div class="intent-usage">
                <span :class="intentReferenceCount(intent.name) ? 'is-used' : ''">
                  {{
                    intentReferenceCount(intent.name)
                      ? `Used by ${intentReferenceCount(intent.name)} admission rule${intentReferenceCount(intent.name) === 1 ? "" : "s"}`
                      : "Not used by an admission rule"
                  }}
                </span>
              </div>
              <details class="orchestration-details">
                <summary>
                  <span>Timing, restart, and event options</span>
                  <small>{{ intentAdvancedSummary(intent) }}</small>
                </summary>
                <div class="advanced-grid">
                  <label class="orchestration-field">
                    <span>Coalesce window</span>
                    <div class="input-with-suffix">
                      <input v-model.number="intent.coalesce_seconds" min="0" type="number" /><span
                        >seconds</span
                      >
                    </div>
                    <small
                      >Wait this long to combine repeated responses. Use 0 for immediate.</small
                    >
                  </label>
                  <label class="orchestration-field">
                    <span>Stop current epoch</span>
                    <select v-model="intent.stop">
                      <option value="cancel">Cancel it</option>
                      <option value="pause">Pause it</option>
                      <option value="none">Leave it running</option>
                    </select>
                  </label>
                  <label class="orchestration-field">
                    <span>Restart from</span>
                    <select v-model="intent.restart_kind">
                      <option value="entry">Pipeline entry</option>
                      <option value="current">Current phase</option>
                      <option value="member">A specific phase</option>
                    </select>
                  </label>
                  <label v-if="intent.restart_kind === 'member'" class="orchestration-field">
                    <span>Restart phase</span>
                    <select v-model="intent.restart_member">
                      <option value="">Choose a phase…</option>
                      <option v-for="member in members" :key="member" :value="member">
                        {{ member }}
                      </option>
                    </select>
                  </label>
                  <label class="orchestration-field">
                    <span>Subject revision field</span>
                    <input
                      v-model="intent.subject_revision_pointer"
                      list="orchestration-pointers"
                      placeholder="Optional, e.g. /subject_revision"
                    />
                  </label>
                  <label v-if="intent.effect === 'signal'" class="orchestration-field">
                    <span>Workflow signal name</span>
                    <input v-model="intent.signal_name" placeholder="Defaults to response name" />
                  </label>
                  <label class="orchestration-check md:col-span-2">
                    <input v-model="intent.allow_self_originated" type="checkbox" />
                    <span
                      ><strong>Accept self-originated events</strong
                      ><small
                        >Allow this response to react to an event created by the orchestration
                        itself.</small
                      ></span
                    >
                  </label>
                </div>
              </details>
            </article>
          </div>

          <div v-else-if="tab === 'Budgets'" class="grid gap-4">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Retry budgets</h3>
                  <HelpBubble label="About failure policies">
                    <strong>Failure handling</strong>
                    <p>
                      Budgets limit retries for a named failure class and choose what happens when
                      the limit is reached.
                    </p>
                  </HelpBubble>
                </div>
              </div>
              <button type="button" class="btn btn-primary btn-sm" @click="addBudget">
                <Icon name="plus" :size="15" />
                Add budget
              </button>
            </header>
            <section
              v-if="budgets.length === 0"
              class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
            >
              <strong class="block text-fg">No retry budgets yet</strong>
              Add a budget when a failure class needs bounded attempts or a recovery handoff.
            </section>
            <article v-for="(budget, index) in budgets" :key="budget.id" class="orchestration-card">
              <div class="orchestration-card-heading">
                <div>
                  <p class="orchestration-eyebrow">Failure policy {{ index + 1 }}</p>
                  <strong>{{ budget.name || "Unnamed failure class" }}</strong>
                </div>
                <button
                  type="button"
                  class="btn btn-ghost btn-sm text-danger-fg"
                  @click="budgets.splice(index, 1)"
                >
                  Remove budget
                </button>
              </div>
              <div class="budget-builder">
                <span>For</span>
                <label class="orchestration-field"
                  ><span>Failure class</span
                  ><input v-model="budget.name" required placeholder="transient"
                /></label>
                <span>allow</span>
                <label class="orchestration-field budget-attempts"
                  ><span>Maximum attempts</span
                  ><input v-model.number="budget.attempts" type="number" min="1" step="1"
                /></label>
                <span>then</span>
                <label class="orchestration-field"
                  ><span>When attempts are used</span
                  ><select v-model="budget.exhausted">
                    <option value="fail">Fail the orchestration</option>
                    <option value="pause">Pause for review</option>
                    <option value="terminate">Terminate the orchestration</option>
                  </select></label
                >
              </div>
              <label class="orchestration-field budget-handoff">
                <span>Optional recovery phase</span>
                <select v-model="budget.handoff">
                  <option value="">No handoff</option>
                  <option v-for="member in members" :key="member" :value="member">
                    Continue with {{ member }}
                  </option>
                </select>
                <small>Hand off to a pipeline phase after this failure policy is exhausted.</small>
              </label>
            </article>
          </div>

          <div v-else-if="tab === 'Phase Mappings'" class="grid gap-4">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Phase mappings</h3>
                  <HelpBubble label="About phase mappings">
                    <strong>Saved phase results</strong>
                    <p>
                      Mappings copy selected workflow results into durable orchestration state.
                      Leave a mapping blank when the phase does not produce that value.
                    </p>
                  </HelpBubble>
                </div>
              </div>
            </header>
            <section
              v-if="phases.length === 0"
              class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
            >
              Add a workflow to this pipeline before configuring phase result mappings.
            </section>
            <section v-else class="phase-flow" aria-label="Pipeline phases">
              <article v-for="(phase, index) in phases" :key="phase.member" class="phase-card">
                <div class="phase-marker" aria-hidden="true">
                  <span>{{ index + 1 }}</span>
                </div>
                <details class="phase-details" :open="phaseMappingCount(phase) > 0 || index === 0">
                  <summary>
                    <span>
                      <strong>{{ phase.member }}</strong>
                      <small>{{
                        phaseMappingCount(phase)
                          ? `${phaseMappingCount(phase)} result fields saved`
                          : "No result data saved yet"
                      }}</small>
                    </span>
                    <span class="phase-summary-badge"
                      >{{ phaseMappingCount(phase) }}/{{ resultPointers.length }}</span
                    >
                  </summary>
                  <div class="mapping-grid">
                    <label
                      v-for="pointer in resultPointers"
                      :key="pointer.key"
                      class="orchestration-field"
                    >
                      <span>{{ pointer.label }}</span>
                      <input
                        v-model="phase[pointer.key]"
                        list="orchestration-pointers"
                        :placeholder="pointer.placeholder"
                      />
                      <small>{{ pointer.description }}</small>
                    </label>
                  </div>
                  <div class="phase-copy-bar">
                    <span>
                      <strong>Reuse these mappings</strong>
                      <small>Copy all five fields over the mappings in every other phase.</small>
                    </span>
                    <button
                      type="button"
                      class="btn btn-sm"
                      :disabled="phases.length < 2"
                      @click="applyPhaseMappingsToAll(phase)"
                    >
                      <Icon name="copy" :size="14" />
                      Apply to every phase
                    </button>
                  </div>
                </details>
              </article>
            </section>
            <p v-if="phaseMappingNotice" class="draft-change-notice" role="status">
              <Icon name="check" :size="14" />
              {{ phaseMappingNotice }}
            </p>
          </div>

          <div v-else-if="tab === 'Workspaces'" class="grid gap-4">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Workspace continuity</h3>
                  <HelpBubble label="About orchestration workspaces">
                    <strong>Working files</strong>
                    <p>
                      Workspace leases keep compatible machine-local files available across phases.
                      Enable them only for phases that need that state.
                    </p>
                  </HelpBubble>
                </div>
              </div>
              <div v-if="phases.length" class="flex flex-wrap gap-2">
                <button type="button" class="btn btn-sm" @click="enableAllWorkspaces">
                  Enable all
                </button>
                <button type="button" class="btn btn-ghost btn-sm" @click="disableAllWorkspaces">
                  Disable all
                </button>
              </div>
            </header>
            <section
              v-if="phases.length === 0"
              class="rounded border border-dashed border-border p-4 text-sm text-fg-muted"
            >
              Add a workflow to this pipeline before configuring phase workspace leases.
            </section>
            <section v-else class="workspace-phase-list">
              <article
                v-for="phase in phases"
                :key="phase.member"
                class="workspace-card"
                :class="{ 'is-enabled': phase.workspace_enabled }"
              >
                <header>
                  <div class="workspace-card-identity">
                    <span class="workspace-icon"><Icon name="folder" :size="16" /></span>
                    <span
                      ><strong>{{ phase.member }}</strong
                      ><small>{{ workspaceSummary(phase) }}</small></span
                    >
                  </div>
                  <label class="switch-control">
                    <input
                      v-model="phase.workspace_enabled"
                      type="checkbox"
                      @change="normalizeWorkspace(phase)"
                    />
                    <span aria-hidden="true"></span>
                    <em>{{ phase.workspace_enabled ? "Enabled" : "Off" }}</em>
                  </label>
                </header>
                <div v-if="phase.workspace_enabled" class="workspace-settings">
                  <label class="orchestration-field">
                    <span>Workspace scope</span>
                    <input v-model="phase.workspace_scope" placeholder="source" />
                    <small>Phases with the same scope can reuse compatible files.</small>
                  </label>
                  <label class="orchestration-field">
                    <span>Lease duration</span>
                    <div class="input-with-suffix">
                      <input
                        v-model.number="phase.lease_seconds"
                        type="number"
                        min="1"
                        step="1"
                      /><span>seconds</span>
                    </div>
                    <small>How long a worker may retain the local materialization.</small>
                  </label>
                  <label class="orchestration-field">
                    <span>If the workspace is unavailable</span>
                    <select v-model="phase.recovery">
                      <option value="replace">Create a replacement</option>
                      <option value="wait">Wait for it to recover</option>
                      <option value="fail">Fail this phase</option>
                    </select>
                  </label>
                  <label class="orchestration-check">
                    <input v-model="phase.reuse" type="checkbox" />
                    <span
                      ><strong>Reuse compatible workspace</strong
                      ><small
                        >Restore the same durable workspace when the scope and worker match.</small
                      ></span
                    >
                  </label>
                  <details class="orchestration-details workspace-requirements">
                    <summary>
                      <span>Worker requirements</span
                      ><small>{{
                        phase.requirementsText === "{}" ? "Any compatible worker" : "Custom labels"
                      }}</small>
                    </summary>
                    <label class="orchestration-field">
                      <span>Requirements JSON</span>
                      <input
                        v-model="phase.requirementsText"
                        placeholder='{ "capability": "git" }'
                      />
                      <small>Advanced worker-selection labels expressed as a JSON object.</small>
                    </label>
                  </details>
                  <div class="workspace-copy-bar">
                    <span>
                      <strong>Copy this workspace policy</strong>
                      <small>Choose whether disabled phases should remain disabled.</small>
                    </span>
                    <div>
                      <button
                        type="button"
                        class="btn btn-sm"
                        :disabled="workspaceCount < 2"
                        @click="applyWorkspacePolicy(phase, false)"
                      >
                        <Icon name="copy" :size="14" />
                        To enabled phases
                      </button>
                      <button
                        type="button"
                        class="btn btn-sm"
                        :disabled="phases.length < 2"
                        @click="applyWorkspacePolicy(phase, true)"
                      >
                        <Icon name="copy" :size="14" />
                        To every phase
                      </button>
                    </div>
                  </div>
                </div>
              </article>
            </section>
            <p v-if="workspaceNotice" class="draft-change-notice" role="status">
              <Icon name="check" :size="14" />
              {{ workspaceNotice }}
            </p>
          </div>

          <div v-else class="grid gap-4">
            <header class="orchestration-section-heading">
              <div class="flex items-center gap-1">
                <h3>Generated policy</h3>
                <HelpBubble label="About orchestration preview">
                  <strong>Advanced policy</strong>
                  <p>This is the generated REXRAP policy saved with the next pipeline revision.</p>
                </HelpBubble>
              </div>
            </header>
            <section class="review-summary">
              <div>
                <strong>{{ routes.length }}</strong
                ><span>admission routes</span>
              </div>
              <div>
                <strong>{{ intents.length }}</strong
                ><span>intents</span>
              </div>
              <div>
                <strong>{{ budgets.length }}</strong
                ><span>retry budgets</span>
              </div>
              <div>
                <strong>{{ phaseMappingTotal }}</strong
                ><span>phase mappings</span>
              </div>
              <div>
                <strong>{{ workspaceCount }}</strong
                ><span>workspaces</span>
              </div>
            </section>
            <pre class="policy-preview">{{ sourcePreview }}</pre>
          </div>
          <datalist id="orchestration-pointers">
            <option v-for="pointer in canonicalPointers" :key="pointer" :value="pointer" />
          </datalist>
        </section>
      </div>
    </template>

    <div class="orchestration-actions">
      <p v-if="enabled && issues.length" class="m-0 text-xs text-danger-fg">
        Review {{ issues.length }} issue{{ issues.length === 1 ? "" : "s" }} before saving.
      </p>
      <span v-else class="flex-1" />
      <button type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button
        type="button"
        class="btn btn-primary"
        :disabled="issues.length > 0"
        :title="issues.length ? 'Resolve the highlighted issues before saving.' : ''"
        @click="save"
      >
        Save pipeline revision
      </button>
    </div>

    <section v-if="disableConfirmOpen" class="orchestration-disable-confirm" role="alert">
      <div>
        <strong>Remove orchestration from this pipeline?</strong>
        <p>
          Saving after this will delete all admission routes, intents, budgets, phase mappings, and
          workspace policies from the next pipeline revision.
        </p>
      </div>
      <div class="flex flex-wrap justify-end gap-2">
        <button type="button" class="btn" @click="disableConfirmOpen = false">Keep enabled</button>
        <button type="button" class="btn btn-danger" @click="confirmDisable">
          Disable and remove configuration
        </button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { createUuid } from "../../../core/utils/uuid";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
import type {
  AdapterKindMetadata,
  IngressAction,
  IngressLifecycle,
  IngressPolicy,
  IngressPredicateOperator,
  JsonRecord,
  JsonValue,
  OrchestrationPolicy,
  Pipeline,
} from "../../../core/domain/models";

const props = defineProps<{ pipeline: Pipeline; adapterKinds: AdapterKindMetadata[] }>();
const emit = defineEmits<{ save: [metadata: JsonRecord]; cancel: [] }>();
const tabs = [
  "Admission Routes",
  "Intents",
  "Budgets",
  "Phase Mappings",
  "Workspaces",
  "Preview",
] as const;
type Tab = (typeof tabs)[number];
type Effect = "terminate" | "suspend" | "resume" | "supersede" | "observe" | "signal";
type RestartKind = "entry" | "current" | "member";
type Exhaustion = "fail" | "pause" | "terminate";
type Recovery = "replace" | "wait" | "fail";
interface ValidationIssue {
  tab: Tab;
  message: string;
}
type IngressRouteWire = Omit<IngressPolicy["routes"][number], "predicates"> & {
  // serde omits empty Vec fields, even though the in-memory Rust model always has the collection.
  predicates?: IngressPolicy["routes"][number]["predicates"];
};
interface PredicateDraft {
  id: string;
  pointer: string;
  operator: IngressPredicateOperator;
  valueText: string;
}
interface RouteDraft {
  id: string;
  event_type: string;
  lifecycle: IngressLifecycle;
  action: IngressAction;
  intent: string;
  predicates: PredicateDraft[];
}
interface IntentDraft {
  id: string;
  name: string;
  effect: Effect;
  priority: number;
  coalesce_seconds: number;
  stop: "pause" | "cancel" | "none";
  restart_kind: RestartKind;
  restart_member: string;
  subject_revision_pointer: string;
  signal_name: string;
  allow_self_originated: boolean;
}
interface BudgetDraft {
  id: string;
  name: string;
  attempts: number;
  exhausted: Exhaustion;
  handoff: string;
}
interface PhaseDraft {
  member: string;
  subject_revision: string;
  resources: string;
  evidence: string;
  failure_class: string;
  correlations: string;
  workspace_enabled: boolean;
  workspace_scope: string;
  lease_seconds: number;
  reuse: boolean;
  recovery: Recovery;
  requirementsText: string;
}

const metadata = props.pipeline.metadata;
const existingIngress = metadata.ingress as
  (Omit<IngressPolicy, "routes"> & { routes: IngressRouteWire[] }) | undefined;
const existingPolicy = metadata.orchestration as OrchestrationPolicy | undefined;
const enabled = ref(Boolean(existingPolicy));
const disableConfirmOpen = ref(false);
const tab = ref<Tab>("Admission Routes");
const phaseMappingNotice = ref("");
const workspaceNotice = ref("");
const scope = ref(existingIngress?.scope ?? "correlations");
const members = props.pipeline.graph.members.map((member) => member.key);
const effects: Effect[] = ["terminate", "suspend", "resume", "supersede", "observe", "signal"];
const resultPointers = [
  {
    key: "subject_revision",
    label: "Subject revision",
    placeholder: "/subject_revision",
    description: "Reject stale results when the source object has moved on.",
  },
  {
    key: "resources",
    label: "Resources",
    placeholder: "/resources",
    description: "Keep the resources produced or changed by this phase.",
  },
  {
    key: "evidence",
    label: "Evidence",
    placeholder: "/evidence",
    description: "Retain verification or review evidence for the binding.",
  },
  {
    key: "failure_class",
    label: "Failure class",
    placeholder: "/failure_class",
    description: "Select the failure policy used when this phase fails.",
  },
  {
    key: "correlations",
    label: "Correlation aliases",
    placeholder: "/correlations",
    description: "Add other keys that should address this orchestration.",
  },
] as const;

const routes = reactive<RouteDraft[]>(
  (existingIngress?.routes ?? []).map((route) => ({
    id: createUuid(),
    event_type: route.event_type,
    lifecycle: route.lifecycle,
    action: route.action,
    intent: route.intent ?? "",
    predicates: (route.predicates ?? []).map((predicate) => ({
      id: createUuid(),
      pointer: predicate.pointer,
      operator: predicate.operator,
      valueText: predicate.value === undefined ? "null" : JSON.stringify(predicate.value),
    })),
  })),
);
const intents = reactive<IntentDraft[]>(
  Object.entries(existingPolicy?.intents ?? {}).map(([name, intent]) => ({
    id: createUuid(),
    name,
    effect: intent.effect,
    priority: intent.priority,
    coalesce_seconds: intent.coalesce_seconds ?? 0,
    stop: intent.stop ?? "cancel",
    restart_kind: intent.restart?.kind ?? "entry",
    restart_member: intent.restart?.member ?? "",
    subject_revision_pointer: intent.subject_revision_pointer ?? "",
    signal_name: intent.signal_name ?? "",
    allow_self_originated: intent.allow_self_originated ?? false,
  })),
);
const budgets = reactive<BudgetDraft[]>(
  Object.entries(existingPolicy?.budgets ?? {}).map(([name, budget]) => ({
    id: createUuid(),
    name,
    attempts: budget.attempts,
    exhausted: budget.exhausted,
    handoff: budget.handoff ?? "",
  })),
);
const phases = reactive<PhaseDraft[]>(
  members.map((member) => {
    const phase = existingPolicy?.phases[member];
    return {
      member,
      subject_revision: phase?.result.subject_revision ?? "",
      resources: phase?.result.resources ?? "",
      evidence: phase?.result.evidence ?? "",
      failure_class: phase?.result.failure_class ?? "",
      correlations: phase?.result.correlations ?? "",
      workspace_enabled: Boolean(phase?.workspace),
      workspace_scope: phase?.workspace?.scope ?? "",
      lease_seconds: phase?.workspace?.lease_seconds ?? 300,
      reuse: phase?.workspace?.reuse ?? false,
      recovery: phase?.workspace?.recovery ?? "replace",
      requirementsText: JSON.stringify(phase?.workspace?.requirements ?? {}, null, 0),
    };
  }),
);

const canonicalEvents = computed(() =>
  [...new Set(props.adapterKinds.flatMap((kind) => kind.event_names))].sort(),
);
const canonicalPointers = computed(() =>
  [...new Set(props.adapterKinds.flatMap((kind) => kind.canonical_pointers))].sort(),
);
const issues = computed(() => validate());
const activeTabIssues = computed(() => issues.value.filter((issue) => issue.tab === tab.value));
const issueTabs = computed(() =>
  tabs.map((item) => ({ tab: item, count: tabIssueCount(item) })).filter((item) => item.count > 0),
);
const sourcePreview = computed(() => renderSource());
const workspaceCount = computed(() => phases.filter((phase) => phase.workspace_enabled).length);
const phaseMappingTotal = computed(() =>
  phases.reduce((total, phase) => total + phaseMappingCount(phase), 0),
);

function tabTitle(item: Tab): string {
  const labels: Record<Tab, string> = {
    "Admission Routes": "Admission routes",
    Intents: "Intents",
    Budgets: "Retry budgets",
    "Phase Mappings": "Phase mappings",
    Workspaces: "Workspaces",
    Preview: "Review",
  };
  return labels[item];
}

function tabDomId(item: Tab): string {
  return `orchestration-${item.toLowerCase().replaceAll(" ", "-")}`;
}

function tabDescription(item: Tab): string {
  const descriptions: Record<Tab, string> = {
    "Admission Routes":
      "Decide which incoming events start, update, or record a correlated execution.",
    Intents:
      "Give active-run responses clear names, outcomes, and priorities so admission rules can reuse them.",
    Budgets:
      "Bound recovery attempts by failure class and choose a safe outcome when attempts run out.",
    "Phase Mappings":
      "Choose which workflow result fields become durable orchestration state after each phase.",
    Workspaces:
      "Keep working files available between selected phases and define what happens when a workspace is unavailable.",
    Preview:
      "Check the complete policy and generated REXRAP before saving a new pipeline revision.",
  };
  return descriptions[item];
}

function tabSummary(item: Tab): string {
  const summaries: Record<Tab, string> = {
    "Admission Routes": `${String(routes.length)} rule${routes.length === 1 ? "" : "s"}`,
    Intents: `${String(intents.length)} intent${intents.length === 1 ? "" : "s"}`,
    Budgets: `${String(budgets.length)} budget${budgets.length === 1 ? "" : "s"}`,
    "Phase Mappings": `${String(phaseMappingTotal.value)} fields saved`,
    Workspaces: `${String(workspaceCount.value)} of ${String(phases.length)} enabled`,
    Preview: issues.value.length ? `${String(issues.value.length)} issues` : "Ready to save",
  };
  return summaries[item];
}

function tabComplete(item: Tab): boolean {
  if (tabIssueCount(item) > 0) {
    return false;
  }

  if (item === "Admission Routes") {
    return Boolean(scope.value.trim() && routes.length);
  }

  return item === "Preview" ? issues.value.length === 0 : true;
}

function actionsFor(lifecycle: IngressLifecycle): IngressAction[] {
  return lifecycle === "unbound"
    ? ["start", "record"]
    : lifecycle === "terminal"
      ? ["requeue", "record"]
      : ["dispatch", "interrupt", "queue", "record"];
}

function actionLabel(action: IngressAction): string {
  const labels: Record<IngressAction, string> = {
    start: "Start a new execution",
    interrupt: "Interrupt the active run",
    queue: "Queue for later",
    record: "Record only",
    requeue: "Start the next generation",
    dispatch: "Apply a named response",
  };
  return labels[action];
}

function routeSummary(route: RouteDraft): string {
  const event = route.event_type || "Unnamed event";
  const lifecycle = {
    unbound: "not started",
    active: "running",
    terminal: "finished",
  }[route.lifecycle];
  return `${event} · ${lifecycle} → ${actionLabel(route.action)}`;
}

function effectLabel(effect: Effect): string {
  const labels: Record<Effect, string> = {
    terminate: "Terminate the execution",
    suspend: "Suspend active work",
    resume: "Resume suspended work",
    supersede: "Replace with a new epoch",
    observe: "Record without control",
    signal: "Send a workflow signal",
  };
  return labels[effect];
}

function effectDescription(effect: Effect): string {
  const descriptions: Record<Effect, string> = {
    terminate: "Stop the correlated execution permanently.",
    suspend: "Pause progress until another response resumes it.",
    resume: "Continue a previously suspended execution.",
    supersede: "Stop the current epoch and restart from the selected point.",
    observe: "Save the event while leaving active work unchanged.",
    signal: "Deliver a named signal to the active workflow.",
  };
  return descriptions[effect];
}

function intentAdvancedSummary(intent: IntentDraft): string {
  const parts = [
    intent.coalesce_seconds ? `${String(intent.coalesce_seconds)}s coalesce` : "immediate",
  ];

  if (intent.restart_kind !== "entry") {
    parts.push(intent.restart_kind === "current" ? "restart current" : "restart a phase");
  }

  if (intent.allow_self_originated) {
    parts.push("self-events allowed");
  }

  return parts.join(" · ");
}

function phaseMappingCount(phase: PhaseDraft): number {
  return resultPointers.filter((pointer) => Boolean(phase[pointer.key])).length;
}

function applyPhaseMappingsToAll(source: PhaseDraft): void {
  for (const phase of phases) {
    if (phase.member === source.member) {
      continue;
    }

    for (const pointer of resultPointers) {
      phase[pointer.key] = source[pointer.key];
    }
  }

  phaseMappingNotice.value = `Copied ${source.member}'s mappings to ${String(phases.length - 1)} other phase${phases.length === 2 ? "" : "s"}. These changes remain unsaved.`;
}

function workspaceSummary(phase: PhaseDraft): string {
  if (!phase.workspace_enabled) {
    return "No files retained for this phase";
  }

  return `${phase.workspace_scope || "Scope required"} · ${formatLease(phase.lease_seconds)} · ${phase.reuse ? "reuse" : "fresh materialization"}`;
}

function formatLease(seconds: number): string {
  if (seconds >= 3600 && seconds % 3600 === 0) {
    return `${String(seconds / 3600)}h lease`;
  }

  if (seconds >= 60 && seconds % 60 === 0) {
    return `${String(seconds / 60)}m lease`;
  }

  return `${String(seconds)}s lease`;
}

function normalizeWorkspace(phase: PhaseDraft): void {
  if (phase.workspace_enabled && !phase.workspace_scope.trim()) {
    phase.workspace_scope = "orchestration-workspace";
  }
}

function enableAllWorkspaces(): void {
  for (const phase of phases) {
    phase.workspace_enabled = true;
    normalizeWorkspace(phase);
  }
}

function disableAllWorkspaces(): void {
  for (const phase of phases) {
    phase.workspace_enabled = false;
  }
}

function applyWorkspacePolicy(source: PhaseDraft, includeDisabled: boolean): void {
  let copied = 0;

  for (const phase of phases) {
    if (phase.member === source.member || (!includeDisabled && !phase.workspace_enabled)) {
      continue;
    }

    phase.workspace_enabled = true;
    phase.workspace_scope = source.workspace_scope;
    phase.lease_seconds = source.lease_seconds;
    phase.reuse = source.reuse;
    phase.recovery = source.recovery;
    phase.requirementsText = source.requirementsText;
    copied += 1;
  }

  workspaceNotice.value = `Copied ${source.member}'s workspace policy to ${String(copied)} other phase${copied === 1 ? "" : "s"}. These changes remain unsaved.`;
}

function normalizeRoute(route: RouteDraft): void {
  if (!actionsFor(route.lifecycle).includes(route.action)) {
    route.action = actionsFor(route.lifecycle)[0];
  }

  if (route.action !== "dispatch") {
    route.intent = "";
  }
}

function addRoute(): void {
  const hasIntent = intents.length > 0;
  routes.push({
    id: createUuid(),
    event_type: canonicalEvents.value[0] ?? "updated",
    lifecycle: hasIntent ? "active" : "unbound",
    action: hasIntent ? "dispatch" : "start",
    intent: hasIntent ? intents[0].name : "",
    predicates: [],
  });
}

function addPredicate(route: RouteDraft): void {
  route.predicates.push({
    id: createUuid(),
    pointer: canonicalPointers.value[0] ?? "/",
    operator: "equal",
    valueText: "null",
  });
}

function addIntent(): void {
  intents.push({
    id: createUuid(),
    name: `intent_${String(intents.length + 1)}`,
    effect: "observe",
    priority: 10 - intents.length,
    coalesce_seconds: 0,
    stop: "cancel",
    restart_kind: "entry",
    restart_member: "",
    subject_revision_pointer: "",
    signal_name: "",
    allow_self_originated: false,
  });
}

function addBudget(): void {
  budgets.push({
    id: createUuid(),
    name: `failure_${String(budgets.length + 1)}`,
    attempts: 1,
    exhausted: "pause",
    handoff: "",
  });
}

function enableConfiguration(): void {
  enabled.value = true;
  tab.value = "Admission Routes";
}

function confirmDisable(): void {
  enabled.value = false;
  disableConfirmOpen.value = false;
}

function tabIssueCount(item: Tab): number {
  return issues.value.filter((issue) => issue.tab === item).length;
}

function intentReferenceCount(name: string): number {
  return routes.filter((route) => route.action === "dispatch" && route.intent === name).length;
}

function intentRemovalHint(name: string): string {
  const count = intentReferenceCount(name);
  return count > 0
    ? `Change the ${String(count)} admission route${count === 1 ? "" : "s"} that use this intent before removing it.`
    : "Remove intent";
}

function removeIntent(index: number): void {
  const intent = intents[index];

  if (intentReferenceCount(intent.name) > 0) {
    return;
  }

  intents.splice(index, 1);
}

function routeActionHint(route: RouteDraft): string {
  if (route.action === "dispatch") {
    return route.intent ? `Dispatches ${route.intent}` : "Select an intent";
  }

  const labels: Record<IngressAction, string> = {
    start: "Starts a new run",
    interrupt: "Interrupts the active run",
    queue: "Queues until the run settles",
    record: "Records without changing the run",
    requeue: "Starts the next generation",
    dispatch: "Dispatches an intent",
  };
  return labels[route.action];
}

function parseJson(text: string): JsonValue {
  return JSON.parse(text) as JsonValue;
}

function pointerValid(pointer: string): boolean {
  return pointer === "" || pointer.startsWith("/");
}

function duplicateNames(values: string[]): Set<string> {
  const seen = new Set<string>();
  const duplicates = new Set<string>();

  for (const value of values.map((item) => item.trim()).filter(Boolean)) {
    if (seen.has(value)) {
      duplicates.add(value);
    }

    seen.add(value);
  }

  return duplicates;
}

function validate(): ValidationIssue[] {
  if (!enabled.value) {
    return [];
  }

  const found: ValidationIssue[] = [];
  const add = (issueTab: Tab, message: string) => found.push({ tab: issueTab, message });

  if (!scope.value.trim()) {
    add("Admission Routes", "Correlation scope is required.");
  }

  const priorities = new Set<number>();
  const names = new Set(intents.map((intent) => intent.name));

  for (const duplicate of duplicateNames(intents.map((intent) => intent.name))) {
    add("Intents", `Intent name “${duplicate}” is duplicated.`);
  }

  for (const intent of intents) {
    if (!intent.name.trim()) {
      add("Intents", "Every intent needs a name.");
    }

    if (!Number.isFinite(intent.priority)) {
      add("Intents", `Intent ${intent.name || "(unnamed)"} needs a numeric priority.`);
    } else if (priorities.has(intent.priority)) {
      add("Intents", `Intent priority ${String(intent.priority)} is duplicated.`);
    }

    priorities.add(intent.priority);

    if (!Number.isFinite(intent.coalesce_seconds) || intent.coalesce_seconds < 0) {
      add("Intents", `Intent ${intent.name || "(unnamed)"} needs a non-negative coalesce window.`);
    }

    if (intent.restart_kind === "member" && !members.includes(intent.restart_member)) {
      add("Intents", `Intent ${intent.name} has an unknown restart member.`);
    }

    if (intent.subject_revision_pointer && !pointerValid(intent.subject_revision_pointer)) {
      add("Intents", `Intent ${intent.name} has an invalid subject revision pointer.`);
    }
  }

  for (const route of routes) {
    if (!route.event_type.trim()) {
      add("Admission Routes", "Every route needs an event name.");
    }

    if (route.action === "dispatch" && !names.has(route.intent)) {
      add("Admission Routes", `Route ${route.event_type || "(unnamed)"} needs a known intent.`);
    }

    for (const predicate of route.predicates) {
      if (!predicate.pointer || !pointerValid(predicate.pointer)) {
        add(
          "Admission Routes",
          `Route ${route.event_type || "(unnamed)"} has an invalid condition pointer.`,
        );
      }

      if (predicate.operator === "exists") {
        continue;
      }

      try {
        parseJson(predicate.valueText);
      } catch {
        add("Admission Routes", `Condition ${predicate.pointer || "(unnamed)"} has invalid JSON.`);
      }
    }
  }

  for (const duplicate of duplicateNames(budgets.map((budget) => budget.name))) {
    add("Budgets", `Budget name “${duplicate}” is duplicated.`);
  }

  for (const budget of budgets) {
    if (!budget.name.trim()) {
      add("Budgets", "Every retry budget needs a failure class.");
    }

    if (!Number.isInteger(budget.attempts) || budget.attempts < 1) {
      add(
        "Budgets",
        `Budget ${budget.name || "(unnamed)"} needs at least one whole-number attempt.`,
      );
    }

    if (budget.handoff && !members.includes(budget.handoff)) {
      add("Budgets", `Budget ${budget.name} has an unknown handoff member.`);
    }
  }

  for (const phase of phases) {
    for (const pointer of resultPointers) {
      if (phase[pointer.key] && !pointerValid(phase[pointer.key])) {
        add("Phase Mappings", `${phase.member} ${pointer.label} pointer is invalid.`);
      }
    }

    if (!phase.workspace_enabled) {
      continue;
    }

    if (!phase.workspace_scope.trim()) {
      add("Workspaces", `${phase.member} workspace scope is required.`);
    }

    if (!Number.isInteger(phase.lease_seconds) || phase.lease_seconds < 1) {
      add("Workspaces", `${phase.member} lease must be at least one whole second.`);
    }

    try {
      parseJson(phase.requirementsText);
    } catch {
      add("Workspaces", `${phase.member} workspace requirements are invalid JSON.`);
    }
  }

  return found.filter(
    (issue, index) =>
      found.findIndex((other) => other.tab === issue.tab && other.message === issue.message) ===
      index,
  );
}

function buildIngress(): IngressPolicy {
  return {
    scope: scope.value.trim(),
    routes: routes.map((route) => ({
      event_type: route.event_type.trim(),
      lifecycle: route.lifecycle,
      action: route.action,
      predicates: route.predicates.map((predicate) => ({
        pointer: predicate.pointer,
        operator: predicate.operator,
        ...(predicate.operator === "exists" ? {} : { value: parseJson(predicate.valueText) }),
      })),
      ...(route.action === "dispatch" ? { intent: route.intent } : {}),
    })),
  };
}

function buildPolicy(): OrchestrationPolicy {
  return {
    intents: Object.fromEntries(
      intents.map((intent) => [
        intent.name,
        {
          effect: intent.effect,
          priority: intent.priority,
          ...(intent.coalesce_seconds > 0 ? { coalesce_seconds: intent.coalesce_seconds } : {}),
          stop: intent.stop,
          restart: {
            kind: intent.restart_kind,
            ...(intent.restart_kind === "member" ? { member: intent.restart_member } : {}),
          },
          ...(intent.subject_revision_pointer
            ? { subject_revision_pointer: intent.subject_revision_pointer }
            : {}),
          ...(intent.effect === "signal" && intent.signal_name
            ? { signal_name: intent.signal_name }
            : {}),
          allow_self_originated: intent.allow_self_originated,
        },
      ]),
    ),
    budgets: Object.fromEntries(
      budgets.map((budget) => [
        budget.name,
        {
          attempts: budget.attempts,
          exhausted: budget.exhausted,
          ...(budget.handoff ? { handoff: budget.handoff } : {}),
        },
      ]),
    ),
    phases: Object.fromEntries(
      phases.map((phase) => [
        phase.member,
        {
          result: {
            ...(phase.subject_revision ? { subject_revision: phase.subject_revision } : {}),
            ...(phase.resources ? { resources: phase.resources } : {}),
            ...(phase.evidence ? { evidence: phase.evidence } : {}),
            ...(phase.failure_class ? { failure_class: phase.failure_class } : {}),
            ...(phase.correlations ? { correlations: phase.correlations } : {}),
          },
          ...(phase.workspace_enabled
            ? {
                workspace: {
                  scope: phase.workspace_scope,
                  requirements: parseJson(phase.requirementsText),
                  lease_seconds: phase.lease_seconds,
                  reuse: phase.reuse,
                  recovery: phase.recovery,
                },
              }
            : {}),
        },
      ]),
    ),
    defaults: existingPolicy?.defaults ?? null,
  };
}

function save(): void {
  if (issues.value.length > 0) {
    tab.value = issues.value[0].tab;
    return;
  }

  const next: JsonRecord = { ...metadata };

  if (enabled.value) {
    next.ingress = buildIngress();
    next.orchestration = buildPolicy();
  } else {
    delete next.ingress;
    delete next.orchestration;
  }

  emit("save", next);
}

function quote(value: string): string {
  return JSON.stringify(value);
}

function renderSource(): string {
  if (!enabled.value) {
    return "# orchestration disabled";
  }

  const lines = [`ingress scope ${quote(scope.value)} {`];

  for (const route of routes) {
    lines.push(`  on ${quote(route.event_type)} when ${route.lifecycle}`);

    for (const predicate of route.predicates) {
      lines.push(
        `    if ${quote(predicate.pointer)} ${predicate.operator} ${predicate.operator === "exists" ? "" : predicate.valueText}`.trimEnd(),
      );
    }

    lines.push(
      `    -> ${route.action}${route.action === "dispatch" ? ` ${quote(route.intent)}` : ""}`,
      "",
    );
  }

  lines.push("}", "", "orchestration {");

  for (const intent of intents) {
    let rendered = `  intent ${quote(intent.name)} effect ${intent.effect} priority ${String(intent.priority)}`;

    if (intent.coalesce_seconds > 0) {
      rendered += ` coalesce ${String(intent.coalesce_seconds)}s`;
    }

    if (intent.stop !== "cancel") {
      rendered += ` stop ${intent.stop}`;
    }

    if (intent.restart_kind === "current") {
      rendered += " restart current";
    } else if (intent.restart_kind === "member") {
      rendered += ` restart ${quote(intent.restart_member)}`;
    }

    if (intent.subject_revision_pointer) {
      rendered += ` revision ${quote(intent.subject_revision_pointer)}`;
    }

    if (intent.effect === "signal" && intent.signal_name) {
      rendered += ` signal ${quote(intent.signal_name)}`;
    }

    if (intent.allow_self_originated) {
      rendered += " allow_self_originated";
    }

    lines.push(rendered);
  }

  for (const budget of budgets) {
    lines.push(
      `  budget ${quote(budget.name)} attempts ${String(budget.attempts)} exhausted ${budget.exhausted}${budget.handoff ? ` via ${quote(budget.handoff)}` : ""}`,
    );
  }

  for (const phase of phases) {
    lines.push("", `  phase ${quote(phase.member)} {`);

    for (const pointer of resultPointers) {
      if (phase[pointer.key]) {
        lines.push(`    ${pointer.key} from ${quote(phase[pointer.key])}`);
      }
    }

    if (phase.workspace_enabled) {
      let workspace = `    workspace scope ${quote(phase.workspace_scope)}`;

      if (phase.reuse) {
        workspace += " reuse";
      }

      if (phase.lease_seconds !== 300) {
        workspace += ` lease ${String(phase.lease_seconds)}s`;
      }

      if (phase.recovery !== "replace") {
        workspace += ` recovery ${phase.recovery}`;
      }

      if (phase.requirementsText.trim() && phase.requirementsText.trim() !== "{}") {
        workspace += ` labels ${phase.requirementsText}`;
      }

      lines.push(workspace);
    }

    lines.push("  }");
  }

  lines.push("}");
  return lines.join("\n");
}
</script>

<style scoped>
.orchestration-editor {
  display: grid;
  gap: var(--space-4);
  min-width: 0;
}

.orchestration-status,
.orchestration-errors,
.orchestration-disable-confirm {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
  padding: var(--space-4);
}

.orchestration-status.is-enabled {
  border-color: color-mix(in srgb, var(--success-fg) 28%, var(--border));
  background: color-mix(in srgb, var(--success-bg) 48%, var(--surface));
}

.orchestration-status-icon {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--accent-text);
}

.orchestration-status h3,
.orchestration-section-heading h3 {
  margin: 2px 0 0;
  color: var(--text);
  font-size: 14px;
  font-weight: 700;
}

.orchestration-status-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: var(--space-2);
}

.orchestration-status-metrics span {
  border: 1px solid color-mix(in srgb, var(--success-fg) 16%, var(--border-subtle));
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--surface) 76%, transparent);
  padding: 3px 8px;
  color: var(--text-muted);
  font-size: 11px;
}

.orchestration-status-metrics strong {
  color: var(--text);
}

.orchestration-status p:not(.orchestration-eyebrow),
.orchestration-section-heading p,
.orchestration-errors p,
.orchestration-disable-confirm p {
  margin: 4px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.orchestration-eyebrow {
  margin: 0;
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.07em;
  text-transform: uppercase;
}

.orchestration-errors,
.orchestration-disable-confirm {
  border-color: color-mix(in srgb, var(--danger-fg) 32%, var(--border));
  background: var(--danger-bg);
}

.orchestration-errors strong,
.orchestration-disable-confirm strong {
  color: var(--danger-fg);
  font-size: 13px;
}

.orchestration-workbench {
  display: grid;
  grid-template-columns: minmax(190px, 230px) minmax(0, 1fr);
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: calc(var(--radius) + 2px);
  background: var(--surface);
}

.orchestration-steps {
  display: flex;
  flex-direction: column;
  gap: 2px;
  border-right: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
  padding: var(--space-3);
}

.orchestration-steps button {
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  border: 1px solid transparent;
  border-radius: var(--radius);
  background: transparent;
  padding: 10px;
  color: var(--text-muted);
  text-align: left;
}

.orchestration-steps button:hover {
  border-color: var(--border-subtle);
  background: var(--surface);
}

.orchestration-steps button.is-active {
  border-color: color-mix(in srgb, var(--accent) 28%, var(--border-subtle));
  background: var(--surface);
  box-shadow: inset 3px 0 0 var(--accent);
  color: var(--text);
}

.orchestration-steps button.has-errors {
  color: var(--danger-fg);
}

.orchestration-step-number {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: 1px solid var(--border-subtle);
  border-radius: 50%;
  background: var(--surface);
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
}

.is-active .orchestration-step-number {
  border-color: var(--accent);
  background: var(--accent);
  color: var(--accent-contrast, white);
}

.orchestration-step-copy {
  display: grid;
  min-width: 0;
}

.orchestration-step-copy strong {
  color: inherit;
  font-size: 12px;
}

.orchestration-step-copy small {
  overflow: hidden;
  color: var(--text-muted);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.orchestration-stage {
  display: grid;
  align-content: start;
  gap: var(--space-4);
  min-width: 0;
  padding: clamp(16px, 2.4vw, 28px);
}

.orchestration-stage-header {
  padding-bottom: var(--space-3);
  border-bottom: 1px solid var(--border-faint);
}

.orchestration-stage-header h2 {
  margin: 2px 0 0;
  color: var(--text);
  font-size: 20px;
  line-height: 1.2;
}

.orchestration-stage-header > p:last-child {
  max-width: 720px;
  margin: 6px 0 0;
  color: var(--text-muted);
  font-size: 13px;
  line-height: 1.5;
}

.orchestration-inline-errors {
  border: 1px solid color-mix(in srgb, var(--danger-fg) 32%, var(--border));
  border-radius: var(--radius);
  background: var(--danger-bg);
  padding: var(--space-3);
  color: var(--danger-fg);
  font-size: 12px;
}

.orchestration-inline-errors ul {
  display: grid;
  gap: 3px;
  margin: 6px 0 0;
  padding-left: 18px;
}

.orchestration-tab-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  border-radius: var(--radius-pill);
  background: var(--danger-bg);
  color: var(--danger-fg);
  font-size: 10px;
  font-weight: 700;
}

.orchestration-section-heading,
.orchestration-card-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.orchestration-section-heading {
  align-items: center;
}

.orchestration-card {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: var(--space-4);
  box-shadow: 0 1px 2px color-mix(in srgb, var(--text) 5%, transparent);
}

.orchestration-card-heading {
  border-bottom: 1px solid var(--border-faint);
  padding-bottom: var(--space-2);
}

.orchestration-card-heading strong {
  color: var(--text);
  font-size: 13px;
}

.orchestration-scope-card {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(220px, 0.8fr);
  align-items: center;
  gap: var(--space-4);
  border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border-subtle));
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--accent) 5%, var(--surface));
  padding: var(--space-4);
}

.orchestration-scope-card strong {
  color: var(--text);
  font-size: 13px;
}

.orchestration-scope-card p {
  margin: 3px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}

.orchestration-field {
  display: grid;
  align-content: start;
  gap: 5px;
  min-width: 0;
  color: var(--text);
  font-size: 11px;
  font-weight: 650;
}

.orchestration-field > small {
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 400;
  line-height: 1.35;
}

.route-builder {
  display: grid;
  grid-template-columns: minmax(150px, 1.1fr) minmax(120px, 0.8fr) minmax(180px, 1.25fr);
  gap: var(--space-3);
}

.route-builder:has(> label:nth-of-type(4)) {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.orchestration-details {
  overflow: hidden;
  border: 1px solid var(--border-faint);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.orchestration-details > summary,
.phase-details > summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: 10px 12px;
  color: var(--text);
  cursor: pointer;
  font-size: 11px;
  font-weight: 650;
  list-style: none;
}

.orchestration-details > summary::-webkit-details-marker,
.phase-details > summary::-webkit-details-marker {
  display: none;
}

.orchestration-details > summary::before,
.phase-details > summary::before {
  content: "+";
  color: var(--text-muted);
  font-size: 15px;
  line-height: 1;
}

.orchestration-details[open] > summary::before,
.phase-details[open] > summary::before {
  content: "−";
}

.orchestration-details > summary > span:first-of-type,
.phase-details > summary > span:first-of-type {
  margin-right: auto;
}

.orchestration-details > summary small,
.phase-details > summary small {
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 400;
}

.orchestration-details > div,
.orchestration-details > label {
  margin: 0 var(--space-3) var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border-faint);
}

.predicate-row {
  display: grid;
  grid-template-columns: auto minmax(130px, 1fr) minmax(120px, 0.65fr) minmax(130px, 1fr) auto;
  align-items: center;
  gap: var(--space-2);
}

.predicate-prefix,
.budget-builder > span {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 650;
}

.intent-primary-grid {
  display: grid;
  grid-template-columns: minmax(150px, 0.85fr) minmax(220px, 1.4fr) minmax(100px, 0.5fr);
  gap: var(--space-3);
}

.intent-usage {
  display: flex;
}

.intent-usage span {
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 4px 8px;
  color: var(--text-muted);
  font-size: 10px;
}

.intent-usage span.is-used {
  background: color-mix(in srgb, var(--success-bg) 64%, var(--surface));
  color: var(--success-fg);
}

.advanced-grid,
.mapping-grid,
.workspace-settings {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
}

.input-with-suffix {
  display: flex;
  min-width: 0;
}

.input-with-suffix input {
  min-width: 0;
  border-radius: var(--radius) 0 0 var(--radius);
}

.input-with-suffix span {
  display: flex;
  align-items: center;
  border: 1px solid var(--border);
  border-left: 0;
  border-radius: 0 var(--radius) var(--radius) 0;
  background: var(--surface-subtle);
  padding: 0 9px;
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 500;
}

.orchestration-check {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  color: var(--text);
  font-size: 11px;
}

.orchestration-check input {
  margin-top: 2px;
}

.orchestration-check > span {
  display: grid;
  gap: 2px;
}

.orchestration-check small {
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 400;
}

.budget-builder {
  display: grid;
  grid-template-columns: auto minmax(150px, 1fr) auto 92px auto minmax(190px, 1.2fr);
  align-items: end;
  gap: var(--space-2);
}

.budget-builder > span {
  padding-bottom: 9px;
}

.budget-handoff {
  max-width: 420px;
}

.phase-flow {
  display: grid;
}

.phase-card {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr);
  gap: var(--space-3);
}

.phase-marker {
  position: relative;
  display: flex;
  justify-content: center;
}

.phase-marker::after {
  position: absolute;
  top: 30px;
  bottom: 0;
  width: 1px;
  background: var(--border-subtle);
  content: "";
}

.phase-card:last-child .phase-marker::after {
  display: none;
}

.phase-marker span {
  z-index: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--border-subtle);
  border-radius: 50%;
  background: var(--surface);
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
}

.phase-details {
  overflow: hidden;
  margin-bottom: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
}

.phase-details > summary > span:first-of-type {
  display: grid;
  min-width: 0;
}

.phase-details > summary strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.phase-summary-badge {
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 3px 7px;
  color: var(--text-muted);
  font-size: 10px;
}

.mapping-grid {
  border-top: 1px solid var(--border-faint);
  padding: var(--space-4);
}

.phase-copy-bar,
.workspace-copy-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  border-top: 1px solid var(--border-faint);
  background: var(--surface-subtle);
  padding: var(--space-3) var(--space-4);
}

.phase-copy-bar > span,
.workspace-copy-bar > span {
  display: grid;
  gap: 2px;
}

.phase-copy-bar strong,
.workspace-copy-bar strong {
  color: var(--text);
  font-size: 11px;
}

.phase-copy-bar small,
.workspace-copy-bar small {
  color: var(--text-muted);
  font-size: 10px;
}

.workspace-copy-bar {
  grid-column: 1 / -1;
  margin: 0 calc(-1 * var(--space-4)) calc(-1 * var(--space-4));
}

.workspace-copy-bar > div {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.draft-change-notice {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0;
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--success-bg) 58%, var(--surface));
  padding: var(--space-2) var(--space-3);
  color: var(--success-fg);
  font-size: 11px;
}

.workspace-phase-list {
  display: grid;
  gap: var(--space-3);
}

.workspace-card {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.workspace-card.is-enabled {
  border-color: color-mix(in srgb, var(--accent) 25%, var(--border-subtle));
  background: var(--surface);
}

.workspace-card > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
}

.workspace-card-identity {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
}

.workspace-card-identity > span:last-child {
  display: grid;
  min-width: 0;
}

.workspace-card-identity strong {
  overflow: hidden;
  color: var(--text);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-card-identity small {
  color: var(--text-muted);
  font-size: 10px;
}

.workspace-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text-muted);
}

.workspace-card.is-enabled .workspace-icon {
  background: color-mix(in srgb, var(--accent) 10%, var(--surface));
  color: var(--accent-text);
}

.workspace-settings {
  border-top: 1px solid var(--border-faint);
  padding: var(--space-4);
}

.workspace-requirements {
  grid-column: 1 / -1;
}

.switch-control {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 7px;
  cursor: pointer;
}

.switch-control input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
}

.switch-control > span {
  position: relative;
  width: 32px;
  height: 18px;
  border-radius: var(--radius-pill);
  background: var(--border);
  transition: background 120ms ease;
}

.switch-control > span::after {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--surface);
  content: "";
  transition: transform 120ms ease;
}

.switch-control input:checked + span {
  background: var(--accent);
}

.switch-control input:checked + span::after {
  transform: translateX(14px);
}

.switch-control input:focus-visible + span {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.switch-control em {
  min-width: 40px;
  color: var(--text-muted);
  font-size: 10px;
  font-style: normal;
  font-weight: 650;
}

.review-summary {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
}

.review-summary > div {
  display: grid;
  gap: 2px;
  border-right: 1px solid var(--border-faint);
  padding: var(--space-3);
}

.review-summary > div:last-child {
  border-right: 0;
}

.review-summary strong {
  color: var(--text);
  font-size: 18px;
}

.review-summary span {
  color: var(--text-muted);
  font-size: 10px;
}

.policy-preview {
  max-height: 32rem;
  overflow: auto;
  margin: 0;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-raised);
  padding: var(--space-4);
  color: var(--text);
  font-size: 11px;
  line-height: 1.55;
}

.orchestration-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  border-top: 1px solid var(--border-subtle);
  background: var(--surface);
  padding-top: var(--space-3);
}

.orchestration-actions > p {
  margin-right: auto;
}

@media (max-width: 920px) {
  .orchestration-workbench {
    grid-template-columns: minmax(0, 1fr);
  }

  .orchestration-steps {
    flex-direction: row;
    overflow-x: auto;
    border-right: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .orchestration-steps button {
    flex: 0 0 150px;
  }

  .orchestration-steps button.is-active {
    box-shadow: inset 0 -3px 0 var(--accent);
  }

  .review-summary {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .review-summary > div:nth-child(3) {
    border-right: 0;
  }

  .review-summary > div:nth-child(n + 4) {
    border-top: 1px solid var(--border-faint);
  }
}

@media (max-width: 700px) {
  .orchestration-status,
  .orchestration-errors,
  .orchestration-disable-confirm,
  .orchestration-section-heading {
    flex-direction: column;
  }

  .orchestration-status > button,
  .orchestration-section-heading > button {
    width: 100%;
  }

  .orchestration-scope-card,
  .route-builder,
  .route-builder:has(> label:nth-of-type(4)),
  .intent-primary-grid,
  .advanced-grid,
  .mapping-grid,
  .workspace-settings {
    grid-template-columns: minmax(0, 1fr);
  }

  .predicate-row {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .predicate-prefix {
    grid-column: 1 / -1;
  }

  .predicate-row > input,
  .predicate-row > select {
    grid-column: 1;
  }

  .predicate-row > button {
    grid-column: 2;
    grid-row: 2;
  }

  .budget-builder {
    grid-template-columns: minmax(0, 1fr);
  }

  .budget-builder > span {
    display: none;
  }

  .workspace-card > header {
    align-items: flex-start;
  }

  .phase-copy-bar,
  .workspace-copy-bar {
    align-items: stretch;
    flex-direction: column;
  }

  .phase-copy-bar > button,
  .workspace-copy-bar > div,
  .workspace-copy-bar button {
    width: 100%;
  }

  .workspace-card-identity small {
    white-space: normal;
  }

  .review-summary {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .review-summary > div,
  .review-summary > div:nth-child(3) {
    border-right: 1px solid var(--border-faint);
    border-top: 1px solid var(--border-faint);
  }

  .review-summary > div:nth-child(odd) {
    border-right: 0;
  }

  .review-summary > div:nth-child(-n + 2) {
    border-top: 0;
  }

  .orchestration-actions {
    flex-wrap: wrap;
  }

  .orchestration-actions > p {
    width: 100%;
  }
}
</style>
