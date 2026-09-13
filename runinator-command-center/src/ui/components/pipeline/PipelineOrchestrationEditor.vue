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
              :id="routeDomId(route.id)"
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
                <span v-if="intentRoutes(intent.name).length === 0"
                  >Not used by an admission rule</span
                >
                <template v-else>
                  <button
                    v-for="route in intentRoutes(intent.name)"
                    :key="route.id"
                    type="button"
                    class="policy-tag is-used"
                    @click="focusRoute(route.id)"
                  >
                    {{ route.event_type || "Unnamed event" }} ·
                    {{ lifecycleLabel(route.lifecycle) }}
                  </button>
                </template>
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

          <div v-else-if="tab === 'Phase Policies'" class="grid gap-5">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Reusable phase policies</h3>
                  <HelpBubble label="About phase policies">
                    <strong>Define once, assign later</strong>
                    <p>
                      Result mappings and workspace policies are reusable within this pipeline.
                      Assign them to phases in the next section.
                    </p>
                  </HelpBubble>
                </div>
              </div>
            </header>
            <section class="policy-library" aria-labelledby="result-profile-heading">
              <header>
                <div>
                  <h4 id="result-profile-heading">Result mappings</h4>
                  <p>Choose which workflow output fields become durable orchestration state.</p>
                </div>
                <button type="button" class="btn btn-primary btn-sm" @click="addResultProfile">
                  <Icon name="plus" :size="14" /> Add mapping
                </button>
              </header>
              <p v-if="resultProfiles.length === 0" class="policy-empty">
                No result mappings. Phases without one do not retain mapped result fields.
              </p>
              <article v-for="profile in resultProfiles" :key="profile.id" class="profile-card">
                <header>
                  <label class="orchestration-field profile-name">
                    <span>Mapping name</span>
                    <input v-model="profile.name" required placeholder="Core results" />
                  </label>
                  <span class="profile-usage">{{ profileUsageLabel("result", profile.id) }}</span>
                  <div class="profile-actions">
                    <button
                      type="button"
                      class="btn btn-sm"
                      @click="duplicateResultProfile(profile)"
                    >
                      <Icon name="copy" :size="14" /> Duplicate
                    </button>
                    <button
                      type="button"
                      class="btn btn-ghost btn-sm text-danger-fg"
                      @click="requestProfileRemoval('result', profile.id)"
                    >
                      Remove
                    </button>
                  </div>
                </header>
                <div class="mapping-grid">
                  <label
                    v-for="pointer in commonResultPointers"
                    :key="pointer.key"
                    class="orchestration-field"
                  >
                    <span>{{ pointer.label }}</span>
                    <input
                      v-model="profile.mapping[pointer.key]"
                      list="orchestration-pointers"
                      :placeholder="pointer.placeholder"
                    />
                    <small>{{ pointer.description }}</small>
                  </label>
                </div>
                <details class="orchestration-details profile-advanced">
                  <summary>
                    <span>Advanced result routing</span>
                    <small>{{ advancedMappingSummary(profile) }}</small>
                  </summary>
                  <div class="mapping-grid">
                    <label
                      v-for="pointer in advancedResultPointers"
                      :key="pointer.key"
                      class="orchestration-field"
                    >
                      <span>{{ pointer.label }}</span>
                      <input
                        v-model="profile.mapping[pointer.key]"
                        list="orchestration-pointers"
                        :placeholder="pointer.placeholder"
                      />
                      <small>{{ pointer.description }}</small>
                    </label>
                  </div>
                </details>
              </article>
            </section>

            <section class="policy-library" aria-labelledby="workspace-profile-heading">
              <header>
                <div>
                  <h4 id="workspace-profile-heading">Workspace policies</h4>
                  <p>Define how compatible working files are retained and recovered.</p>
                </div>
                <button type="button" class="btn btn-primary btn-sm" @click="addWorkspaceProfile">
                  <Icon name="plus" :size="14" /> Add workspace policy
                </button>
              </header>
              <p v-if="workspaceProfiles.length === 0" class="policy-empty">
                No workspace policies. Phases without one use a fresh working environment.
              </p>
              <article v-for="profile in workspaceProfiles" :key="profile.id" class="profile-card">
                <header>
                  <label class="orchestration-field profile-name">
                    <span>Policy name</span>
                    <input v-model="profile.name" required placeholder="Shared source" />
                  </label>
                  <span class="profile-usage">{{
                    profileUsageLabel("workspace", profile.id)
                  }}</span>
                  <div class="profile-actions">
                    <button
                      type="button"
                      class="btn btn-sm"
                      @click="duplicateWorkspaceProfile(profile)"
                    >
                      <Icon name="copy" :size="14" /> Duplicate
                    </button>
                    <button
                      type="button"
                      class="btn btn-ghost btn-sm text-danger-fg"
                      @click="requestProfileRemoval('workspace', profile.id)"
                    >
                      Remove
                    </button>
                  </div>
                </header>
                <div class="workspace-settings">
                  <label class="orchestration-field">
                    <span>Workspace scope</span>
                    <input v-model="profile.scope" placeholder="source" />
                    <small>Phases with the same scope can reuse compatible files.</small>
                  </label>
                  <label class="orchestration-field">
                    <span>Lease duration</span>
                    <div class="input-with-suffix">
                      <input
                        v-model.number="profile.lease_seconds"
                        type="number"
                        min="1"
                        step="1"
                      /><span>seconds</span>
                    </div>
                    <small>How long a worker may retain the local materialization.</small>
                  </label>
                  <label class="orchestration-field">
                    <span>If the workspace is unavailable</span>
                    <select v-model="profile.recovery">
                      <option value="replace">Create a replacement</option>
                      <option value="wait">Wait for it to recover</option>
                      <option value="fail">Fail this phase</option>
                    </select>
                  </label>
                  <label class="orchestration-check">
                    <input v-model="profile.reuse" type="checkbox" />
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
                        profile.requirementsText === "{}"
                          ? "Any compatible worker"
                          : "Custom labels"
                      }}</small>
                    </summary>
                    <label class="orchestration-field">
                      <span>Requirements JSON</span>
                      <input
                        v-model="profile.requirementsText"
                        placeholder='{ "capability": "git" }'
                      />
                      <small>Advanced worker-selection labels expressed as a JSON object.</small>
                    </label>
                  </details>
                </div>
              </article>
            </section>
          </div>

          <div v-else-if="tab === 'Assignments'" class="grid gap-5">
            <header class="orchestration-section-heading">
              <div>
                <div class="flex items-center gap-1">
                  <h3>Apply policies to phases</h3>
                  <HelpBubble label="About phase assignments">
                    <strong>One tag of each type</strong>
                    <p>
                      A phase may use one result mapping and one workspace policy. Reassigning a
                      phase moves it from the previous profile.
                    </p>
                  </HelpBubble>
                </div>
              </div>
            </header>
            <p v-if="members.length === 0" class="policy-empty">
              Add a workflow to this pipeline before assigning phase policies.
            </p>
            <section v-else class="assignment-library" aria-labelledby="result-assignment-heading">
              <header>
                <div>
                  <h4 id="result-assignment-heading">Result mappings</h4>
                  <p>{{ unassignedResultMembers.length }} phases currently have no mapping.</p>
                </div>
                <button type="button" class="btn btn-sm" @click="tab = 'Phase Policies'">
                  Edit mappings
                </button>
              </header>
              <p v-if="resultProfiles.length === 0" class="policy-empty">
                Create a result mapping before assigning one.
              </p>
              <article
                v-for="profile in resultProfiles"
                :key="profile.id"
                class="assignment-profile"
              >
                <div class="assignment-profile-heading">
                  <div>
                    <strong>{{ profile.name || "Unnamed mapping" }}</strong>
                    <small>{{ assignedResultMembers(profile.id).length }} assigned</small>
                  </div>
                  <div class="policy-tag-list">
                    <button
                      v-for="member in assignedResultMembers(profile.id)"
                      :key="member"
                      type="button"
                      class="policy-tag"
                      :aria-label="`Remove ${profile.name} from ${member}`"
                      @click="clearResultAssignment(member)"
                    >
                      {{ member }} <span aria-hidden="true">×</span>
                    </button>
                    <span v-if="assignedResultMembers(profile.id).length === 0">Not assigned</span>
                  </div>
                </div>
                <details class="assignment-picker">
                  <summary>Choose phases</summary>
                  <div>
                    <label v-for="member in members" :key="member">
                      <input
                        type="checkbox"
                        :checked="assignmentFor(member).result_mapping_id === profile.id"
                        @change="toggleResultAssignment(member, profile.id)"
                      />
                      <span>{{ member }}</span>
                      <small>{{ resultAssignmentOwner(member, profile.id) }}</small>
                    </label>
                  </div>
                </details>
              </article>
              <div v-if="unassignedResultMembers.length" class="unassigned-row">
                <strong>No result mapping</strong>
                <div class="policy-tag-list">
                  <span v-for="member in unassignedResultMembers" :key="member" class="policy-tag">
                    {{ member }}
                  </span>
                </div>
              </div>
            </section>

            <section class="assignment-library" aria-labelledby="workspace-assignment-heading">
              <header>
                <div>
                  <h4 id="workspace-assignment-heading">Workspace policies</h4>
                  <p>{{ unassignedWorkspaceMembers.length }} phases use no retained workspace.</p>
                </div>
                <button type="button" class="btn btn-sm" @click="tab = 'Phase Policies'">
                  Edit workspace policies
                </button>
              </header>
              <p v-if="workspaceProfiles.length === 0" class="policy-empty">
                Create a workspace policy before assigning one.
              </p>
              <article
                v-for="profile in workspaceProfiles"
                :key="profile.id"
                class="assignment-profile"
              >
                <div class="assignment-profile-heading">
                  <div>
                    <strong>{{ profile.name || "Unnamed workspace policy" }}</strong>
                    <small>{{ assignedWorkspaceMembers(profile.id).length }} assigned</small>
                  </div>
                  <div class="policy-tag-list">
                    <button
                      v-for="member in assignedWorkspaceMembers(profile.id)"
                      :key="member"
                      type="button"
                      class="policy-tag"
                      :aria-label="`Remove ${profile.name} from ${member}`"
                      @click="clearWorkspaceAssignment(member)"
                    >
                      {{ member }} <span aria-hidden="true">×</span>
                    </button>
                    <span v-if="assignedWorkspaceMembers(profile.id).length === 0"
                      >Not assigned</span
                    >
                  </div>
                </div>
                <details class="assignment-picker">
                  <summary>Choose phases</summary>
                  <div>
                    <label v-for="member in members" :key="member">
                      <input
                        type="checkbox"
                        :checked="assignmentFor(member).workspace_policy_id === profile.id"
                        @change="toggleWorkspaceAssignment(member, profile.id)"
                      />
                      <span>{{ member }}</span>
                      <small>{{ workspaceAssignmentOwner(member, profile.id) }}</small>
                    </label>
                  </div>
                </details>
              </article>
              <div v-if="unassignedWorkspaceMembers.length" class="unassigned-row">
                <strong>No workspace policy</strong>
                <div class="policy-tag-list">
                  <span
                    v-for="member in unassignedWorkspaceMembers"
                    :key="member"
                    class="policy-tag"
                  >
                    {{ member }}
                  </span>
                </div>
              </div>
            </section>
            <p v-if="assignmentNotice" class="draft-change-notice" role="status">
              <Icon name="check" :size="14" />
              {{ assignmentNotice }}
            </p>
          </div>

          <div v-else class="grid gap-4">
            <header class="orchestration-section-heading">
              <div class="flex items-center gap-1">
                <h3>Compiled runtime policy</h3>
                <HelpBubble label="About orchestration preview">
                  <strong>Expanded runtime view</strong>
                  <p>
                    This is the compiled REXRAP policy saved with the next pipeline revision.
                    Profile assignments are expanded back into each phase.
                  </p>
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
                <strong>{{ resultProfiles.length }}</strong
                ><span>mapping profiles</span>
              </div>
              <div>
                <strong>{{ workspaceProfiles.length }}</strong
                ><span>workspace profiles</span>
              </div>
              <div>
                <strong>{{ assignmentCount }}</strong
                ><span>phase assignments</span>
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

    <section v-if="pendingProfileRemoval" class="orchestration-disable-confirm" role="alert">
      <div>
        <strong>Remove {{ pendingProfileRemoval.name }}?</strong>
        <p>
          This profile is assigned to {{ pendingProfileRemoval.count }} phase{{
            pendingProfileRemoval.count === 1 ? "" : "s"
          }}. Removing it will clear those draft assignments.
        </p>
      </div>
      <div class="flex flex-wrap justify-end gap-2">
        <button type="button" class="btn" @click="pendingProfileRemoval = null">
          Keep profile
        </button>
        <button type="button" class="btn btn-danger" @click="confirmProfileRemoval">
          Remove and unassign
        </button>
      </div>
    </section>

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
import { computed, nextTick, reactive, ref } from "vue";
import { asJsonRecord } from "../../../core/domain/json";
import {
  expandPhasePolicyProfiles,
  loadPhasePolicyProfiles,
  phasePolicyProfilesMetadata,
  resultMappingKeys,
  type PhaseProfileAssignment,
  type PhasePolicyProfiles,
  type ResultMappingKey,
} from "../../../core/services";
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
  ResultMapping,
} from "../../../core/domain/models";

const props = defineProps<{ pipeline: Pipeline; adapterKinds: AdapterKindMetadata[] }>();
const emit = defineEmits<{ save: [metadata: JsonRecord]; cancel: [] }>();
const tabs = [
  "Admission Routes",
  "Intents",
  "Budgets",
  "Phase Policies",
  "Assignments",
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
type ResultMappingDraft = Record<ResultMappingKey, string>;
interface ResultProfileDraft {
  id: string;
  name: string;
  mapping: ResultMappingDraft;
}
interface WorkspaceProfileDraft {
  id: string;
  name: string;
  scope: string;
  lease_seconds: number;
  reuse: boolean;
  recovery: Recovery;
  requirementsText: string;
}
interface PendingProfileRemoval {
  kind: "result" | "workspace";
  id: string;
  name: string;
  count: number;
}

const metadata = props.pipeline.metadata;
const existingIngress = metadata.ingress as
  (Omit<IngressPolicy, "routes"> & { routes: IngressRouteWire[] }) | undefined;
const existingPolicy = metadata.orchestration as OrchestrationPolicy | undefined;
const enabled = ref(Boolean(existingPolicy));
const disableConfirmOpen = ref(false);
const tab = ref<Tab>("Admission Routes");
const assignmentNotice = ref("");
const pendingProfileRemoval = ref<PendingProfileRemoval | null>(null);
const scope = ref(existingIngress?.scope ?? "correlations");
const members = props.pipeline.graph.members.map((member) => member.key);
const effects: Effect[] = ["terminate", "suspend", "resume", "supersede", "observe", "signal"];
const resultPointers: {
  key: ResultMappingKey;
  label: string;
  placeholder: string;
  description: string;
}[] = [
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
  {
    key: "resources_patch",
    label: "Resources patch",
    placeholder: "/resources_patch",
    description: "Merge selected fields into retained resources without replacing other context.",
  },
  {
    key: "next_member",
    label: "Next phase",
    placeholder: "/next_member",
    description: "Read the next declared pipeline phase from the workflow result.",
  },
];
const commonResultPointers = resultPointers.slice(0, 5);
const advancedResultPointers = resultPointers.slice(5);

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
const loadedProfiles = loadPhasePolicyProfiles(
  members,
  existingPolicy?.phases ?? {},
  metadata.orchestration_authoring,
);
const resultProfiles = reactive<ResultProfileDraft[]>(
  loadedProfiles.result_mappings.map((profile) => ({
    id: profile.id,
    name: profile.name,
    mapping: mappingDraft(profile.mapping),
  })),
);
const workspaceProfiles = reactive<WorkspaceProfileDraft[]>(
  loadedProfiles.workspace_policies.map((profile) => ({
    id: profile.id,
    name: profile.name,
    scope: profile.policy.scope,
    lease_seconds: profile.policy.lease_seconds,
    reuse: profile.policy.reuse,
    recovery: profile.policy.recovery,
    requirementsText: JSON.stringify(profile.policy.requirements ?? {}),
  })),
);
const assignments = reactive<Record<string, PhaseProfileAssignment>>(
  Object.fromEntries(members.map((member) => [member, { ...loadedProfiles.assignments[member] }])),
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
const unassignedResultMembers = computed(() =>
  members.filter((member) => !assignments[member].result_mapping_id),
);
const unassignedWorkspaceMembers = computed(() =>
  members.filter((member) => !assignments[member].workspace_policy_id),
);
const workspaceCount = computed(() => members.length - unassignedWorkspaceMembers.value.length);
const assignmentCount = computed(
  () =>
    members.filter((member) => assignments[member].result_mapping_id).length +
    members.filter((member) => assignments[member].workspace_policy_id).length,
);

function tabTitle(item: Tab): string {
  const labels: Record<Tab, string> = {
    "Admission Routes": "Admission routes",
    Intents: "Intents",
    Budgets: "Retry budgets",
    "Phase Policies": "Phase policies",
    Assignments: "Assignments",
    Preview: "Preview",
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
    "Phase Policies":
      "Define reusable result mappings and workspace behavior once for this pipeline.",
    Assignments: "Apply mapping and workspace policy tags to one or more pipeline phases.",
    Preview:
      "Check the compiled runtime policy and expanded phase assignments before saving a new pipeline revision.",
  };
  return descriptions[item];
}

function tabSummary(item: Tab): string {
  const summaries: Record<Tab, string> = {
    "Admission Routes": `${String(routes.length)} rule${routes.length === 1 ? "" : "s"}`,
    Intents: `${String(intents.length)} intent${intents.length === 1 ? "" : "s"}`,
    Budgets: `${String(budgets.length)} budget${budgets.length === 1 ? "" : "s"}`,
    "Phase Policies": `${String(resultProfiles.length + workspaceProfiles.length)} profiles`,
    Assignments: `${String(assignmentCount.value)} applied tags`,
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

function mappingDraft(mapping: ResultMapping): ResultMappingDraft {
  return Object.fromEntries(
    resultMappingKeys.map((key) => [key, typeof mapping[key] === "string" ? mapping[key] : ""]),
  ) as ResultMappingDraft;
}

function assignmentFor(member: string): PhaseProfileAssignment {
  assignments[member] ??= {};
  return assignments[member];
}

function assignedResultMembers(profileId: string): string[] {
  return members.filter((member) => assignments[member].result_mapping_id === profileId);
}

function assignedWorkspaceMembers(profileId: string): string[] {
  return members.filter((member) => assignments[member].workspace_policy_id === profileId);
}

function profileUsageLabel(kind: PendingProfileRemoval["kind"], profileId: string): string {
  const count =
    kind === "result"
      ? assignedResultMembers(profileId).length
      : assignedWorkspaceMembers(profileId).length;
  return count ? `Affects ${String(count)} phase${count === 1 ? "" : "s"}` : "Not assigned";
}

function advancedMappingSummary(profile: ResultProfileDraft): string {
  const count = advancedResultPointers.filter((pointer) => profile.mapping[pointer.key]).length;
  return count ? `${String(count)} configured` : "Optional";
}

function addResultProfile(): void {
  resultProfiles.push({
    id: createUuid(),
    name: nextProfileName(
      "Mapping",
      resultProfiles.map((profile) => profile.name),
    ),
    mapping: mappingDraft({}),
  });
}

function duplicateResultProfile(source: ResultProfileDraft): void {
  resultProfiles.push({
    id: createUuid(),
    name: nextProfileName(
      `${source.name || "Mapping"} copy`,
      resultProfiles.map((profile) => profile.name),
    ),
    mapping: { ...source.mapping },
  });
  assignmentNotice.value = "Duplicated the mapping as an unassigned draft profile.";
}

function addWorkspaceProfile(): void {
  workspaceProfiles.push({
    id: createUuid(),
    name: nextProfileName(
      "Workspace",
      workspaceProfiles.map((profile) => profile.name),
    ),
    scope: "orchestration-workspace",
    lease_seconds: 300,
    reuse: false,
    recovery: "replace",
    requirementsText: "{}",
  });
}

function duplicateWorkspaceProfile(source: WorkspaceProfileDraft): void {
  workspaceProfiles.push({
    ...source,
    id: createUuid(),
    name: nextProfileName(
      `${source.name || "Workspace"} copy`,
      workspaceProfiles.map((profile) => profile.name),
    ),
  });
  assignmentNotice.value = "Duplicated the workspace policy as an unassigned draft profile.";
}

function nextProfileName(base: string, existingNames: string[]): string {
  const names = new Set(existingNames);

  if (!names.has(base)) {
    return base;
  }

  let suffix = 2;

  while (names.has(`${base} ${String(suffix)}`)) {
    suffix += 1;
  }

  return `${base} ${String(suffix)}`;
}

function toggleResultAssignment(member: string, profileId: string): void {
  const assignment = assignmentFor(member);
  const previousId = assignment.result_mapping_id;

  if (previousId === profileId) {
    clearResultAssignment(member);
    return;
  }

  assignment.result_mapping_id = profileId;
  assignmentNotice.value = previousId
    ? `Moved ${member} to ${profileName("result", profileId)}. These changes remain unsaved.`
    : `Assigned ${profileName("result", profileId)} to ${member}. These changes remain unsaved.`;
}

function toggleWorkspaceAssignment(member: string, profileId: string): void {
  const assignment = assignmentFor(member);
  const previousId = assignment.workspace_policy_id;

  if (previousId === profileId) {
    clearWorkspaceAssignment(member);
    return;
  }

  assignment.workspace_policy_id = profileId;
  assignmentNotice.value = previousId
    ? `Moved ${member} to ${profileName("workspace", profileId)}. These changes remain unsaved.`
    : `Assigned ${profileName("workspace", profileId)} to ${member}. These changes remain unsaved.`;
}

function clearResultAssignment(member: string): void {
  delete assignmentFor(member).result_mapping_id;
  assignmentNotice.value = `Removed the result mapping from ${member}. These changes remain unsaved.`;
}

function clearWorkspaceAssignment(member: string): void {
  delete assignmentFor(member).workspace_policy_id;
  assignmentNotice.value = `Removed the workspace policy from ${member}. These changes remain unsaved.`;
}

function resultAssignmentOwner(member: string, profileId: string): string {
  const assignedId = assignments[member].result_mapping_id;
  return assignmentOwnerLabel("result", assignedId, profileId);
}

function workspaceAssignmentOwner(member: string, profileId: string): string {
  const assignedId = assignments[member].workspace_policy_id;
  return assignmentOwnerLabel("workspace", assignedId, profileId);
}

function assignmentOwnerLabel(
  kind: PendingProfileRemoval["kind"],
  assignedId: string | undefined,
  profileId: string,
): string {
  if (assignedId === profileId) {
    return "Assigned here";
  }

  return assignedId ? `Currently: ${profileName(kind, assignedId)}` : "Not assigned";
}

function profileName(kind: PendingProfileRemoval["kind"], profileId: string): string {
  const profiles = kind === "result" ? resultProfiles : workspaceProfiles;
  return profiles.find((profile) => profile.id === profileId)?.name ?? "Unnamed profile";
}

function requestProfileRemoval(kind: PendingProfileRemoval["kind"], profileId: string): void {
  const count =
    kind === "result"
      ? assignedResultMembers(profileId).length
      : assignedWorkspaceMembers(profileId).length;

  if (count === 0) {
    removeProfile(kind, profileId);
    return;
  }

  pendingProfileRemoval.value = {
    kind,
    id: profileId,
    name: profileName(kind, profileId),
    count,
  };
}

function confirmProfileRemoval(): void {
  const pending = pendingProfileRemoval.value;

  if (!pending) {
    return;
  }

  removeProfile(pending.kind, pending.id);
  pendingProfileRemoval.value = null;
}

function removeProfile(kind: PendingProfileRemoval["kind"], profileId: string): void {
  const profiles = kind === "result" ? resultProfiles : workspaceProfiles;
  const index = profiles.findIndex((profile) => profile.id === profileId);

  if (index >= 0) {
    profiles.splice(index, 1);
  }

  for (const member of members) {
    if (kind === "result" && assignments[member].result_mapping_id === profileId) {
      delete assignments[member].result_mapping_id;
    }

    if (kind === "workspace" && assignments[member].workspace_policy_id === profileId) {
      delete assignments[member].workspace_policy_id;
    }
  }

  assignmentNotice.value = "Removed the profile and cleared its draft assignments.";
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
  return intentRoutes(name).length;
}

function intentRoutes(name: string): RouteDraft[] {
  return routes.filter((route) => route.action === "dispatch" && route.intent === name);
}

function lifecycleLabel(lifecycle: IngressLifecycle): string {
  return {
    unbound: "not started",
    active: "running",
    terminal: "finished",
  }[lifecycle];
}

function routeDomId(id: string): string {
  return `orchestration-route-${id}`;
}

async function focusRoute(id: string): Promise<void> {
  tab.value = "Admission Routes";
  await nextTick();
  document.getElementById(routeDomId(id))?.scrollIntoView({ behavior: "smooth", block: "center" });
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

  for (const duplicate of duplicateNames(resultProfiles.map((profile) => profile.name))) {
    add("Phase Policies", `Result mapping name “${duplicate}” is duplicated.`);
  }

  for (const profile of resultProfiles) {
    if (!profile.name.trim()) {
      add("Phase Policies", "Every result mapping needs a name.");
    }

    for (const pointer of resultPointers) {
      if (profile.mapping[pointer.key] && !pointerValid(profile.mapping[pointer.key])) {
        add(
          "Phase Policies",
          `${profile.name || "Unnamed mapping"} has an invalid ${pointer.label.toLowerCase()} pointer.`,
        );
      }
    }
  }

  for (const duplicate of duplicateNames(workspaceProfiles.map((profile) => profile.name))) {
    add("Phase Policies", `Workspace policy name “${duplicate}” is duplicated.`);
  }

  for (const profile of workspaceProfiles) {
    if (!profile.name.trim()) {
      add("Phase Policies", "Every workspace policy needs a name.");
    }

    if (!profile.scope.trim()) {
      add("Phase Policies", `${profile.name || "Unnamed workspace policy"} needs a scope.`);
    }

    if (!Number.isInteger(profile.lease_seconds) || profile.lease_seconds < 1) {
      add(
        "Phase Policies",
        `${profile.name || "Unnamed workspace policy"} needs a positive whole-number lease.`,
      );
    }

    try {
      parseJson(profile.requirementsText);
    } catch {
      add(
        "Phase Policies",
        `${profile.name || "Unnamed workspace policy"} has invalid requirements JSON.`,
      );
    }
  }

  const resultIds = new Set(resultProfiles.map((profile) => profile.id));
  const workspaceIds = new Set(workspaceProfiles.map((profile) => profile.id));

  for (const member of members) {
    const assignment = assignments[member];

    if (assignment.result_mapping_id && !resultIds.has(assignment.result_mapping_id)) {
      add("Assignments", `${member} references a missing result mapping.`);
    }

    if (assignment.workspace_policy_id && !workspaceIds.has(assignment.workspace_policy_id)) {
      add("Assignments", `${member} references a missing workspace policy.`);
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

function currentPhaseProfiles(allowInvalidRequirements = false): PhasePolicyProfiles {
  return {
    result_mappings: resultProfiles.map((profile) => ({
      id: profile.id,
      name: profile.name.trim(),
      mapping: Object.fromEntries(
        resultMappingKeys.flatMap((key) => {
          const value = profile.mapping[key].trim();
          return value ? [[key, value]] : [];
        }),
      ),
    })),
    workspace_policies: workspaceProfiles.map((profile) => ({
      id: profile.id,
      name: profile.name.trim(),
      policy: {
        scope: profile.scope.trim(),
        requirements: allowInvalidRequirements
          ? parseJsonOrEmpty(profile.requirementsText)
          : parseJson(profile.requirementsText),
        lease_seconds: profile.lease_seconds,
        reuse: profile.reuse,
        recovery: profile.recovery,
      },
    })),
    assignments: Object.fromEntries(
      members.map((member) => [member, { ...assignmentFor(member) }]),
    ),
  };
}

function parseJsonOrEmpty(value: string): JsonValue {
  try {
    return parseJson(value);
  } catch {
    return {};
  }
}

function buildPolicy(): OrchestrationPolicy {
  const profiles = currentPhaseProfiles();
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
    phases: expandPhasePolicyProfiles(members, profiles),
    ...(existingPolicy?.entry_member ? { entry_member: existingPolicy.entry_member } : {}),
    ...(existingPolicy?.max_epochs !== undefined && existingPolicy.max_epochs !== null
      ? { max_epochs: existingPolicy.max_epochs }
      : {}),
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
    const authoring = asJsonRecord(metadata.orchestration_authoring);
    next.ingress = buildIngress();
    next.orchestration = buildPolicy();
    next.orchestration_authoring = {
      ...authoring,
      schema_version: 2,
      phase_profiles: phasePolicyProfilesMetadata(currentPhaseProfiles()),
    };
  } else {
    delete next.ingress;
    delete next.orchestration;
    delete next.orchestration_authoring;
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

  const previewPhases = expandPhasePolicyProfiles(members, currentPhaseProfiles(true));

  for (const [member, phase] of Object.entries(previewPhases)) {
    lines.push("", `  phase ${quote(member)} {`);

    for (const pointer of resultPointers) {
      const value = phase.result[pointer.key];

      if (value) {
        lines.push(`    ${pointer.key} from ${quote(value)}`);
      }
    }

    if (phase.workspace) {
      let workspace = `    workspace scope ${quote(phase.workspace.scope)}`;

      if (phase.workspace.reuse) {
        workspace += " reuse";
      }

      if (phase.workspace.lease_seconds !== 300) {
        workspace += ` lease ${String(phase.workspace.lease_seconds)}s`;
      }

      if (phase.workspace.recovery !== "replace") {
        workspace += ` recovery ${phase.workspace.recovery}`;
      }

      const requirements = JSON.stringify(phase.workspace.requirements);

      if (requirements !== "{}") {
        workspace += ` labels ${requirements}`;
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

.orchestration-details > summary {
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

.orchestration-details > summary::-webkit-details-marker {
  display: none;
}

.orchestration-details > summary::before {
  content: "+";
  color: var(--text-muted);
  font-size: 15px;
  line-height: 1;
}

.orchestration-details[open] > summary::before {
  content: "−";
}

.orchestration-details > summary > span:first-of-type {
  margin-right: auto;
}

.orchestration-details > summary small {
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
  flex-wrap: wrap;
  gap: 6px;
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

.mapping-grid {
  border-top: 1px solid var(--border-faint);
  padding: var(--space-4);
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

.policy-library,
.assignment-library {
  display: grid;
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
}

.policy-library > header,
.assignment-library > header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
  background: var(--surface-subtle);
  padding: var(--space-4);
}

.policy-library h4,
.assignment-library h4 {
  margin: 0;
  color: var(--text);
  font-size: 13px;
}

.policy-library > header p,
.assignment-library > header p {
  margin: 3px 0 0;
  color: var(--text-muted);
  font-size: 11px;
}

.policy-empty {
  margin: 0;
  border-top: 1px dashed var(--border-subtle);
  padding: var(--space-4);
  color: var(--text-muted);
  font-size: 11px;
}

.profile-card,
.assignment-profile,
.unassigned-row {
  border-top: 1px solid var(--border-subtle);
}

.profile-card > header {
  display: grid;
  grid-template-columns: minmax(180px, 1fr) auto auto;
  align-items: end;
  gap: var(--space-3);
  padding: var(--space-4);
}

.profile-name {
  max-width: 420px;
}

.profile-usage {
  align-self: center;
  border-radius: var(--radius-pill);
  background: var(--surface-subtle);
  padding: 4px 8px;
  color: var(--text-muted);
  font-size: 10px;
  white-space: nowrap;
}

.profile-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.profile-card .mapping-grid,
.profile-card .workspace-settings {
  background: color-mix(in srgb, var(--surface-subtle) 45%, var(--surface));
}

.profile-advanced {
  margin: 0 var(--space-4) var(--space-4);
}

.profile-advanced .mapping-grid {
  margin: 0 var(--space-3) var(--space-3);
  padding: var(--space-3) 0 0;
}

.assignment-profile {
  padding: var(--space-4);
}

.assignment-profile-heading,
.unassigned-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.assignment-profile-heading > div:first-child {
  display: grid;
  min-width: 150px;
}

.assignment-profile-heading strong,
.unassigned-row > strong {
  color: var(--text);
  font-size: 12px;
}

.assignment-profile-heading small {
  color: var(--text-muted);
  font-size: 10px;
}

.policy-tag-list {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
  color: var(--text-muted);
  font-size: 10px;
}

.policy-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid color-mix(in srgb, var(--accent) 24%, var(--border-subtle));
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--accent) 7%, var(--surface));
  padding: 4px 7px;
  color: var(--accent-text);
  font-size: 10px;
  font-weight: 650;
}

button.policy-tag {
  cursor: pointer;
}

button.policy-tag:hover {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, var(--surface));
}

.assignment-picker {
  margin-top: var(--space-3);
  border: 1px solid var(--border-faint);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.assignment-picker > summary {
  padding: 8px 10px;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 10px;
  font-weight: 650;
}

.assignment-picker > div {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
  border-top: 1px solid var(--border-faint);
  padding: var(--space-3);
}

.assignment-picker label {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 2px 8px;
  border: 1px solid var(--border-faint);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 8px;
  color: var(--text);
  font-size: 11px;
}

.assignment-picker label input {
  grid-row: 1 / 3;
  margin-top: 2px;
}

.assignment-picker label small {
  overflow: hidden;
  color: var(--text-muted);
  font-size: 9px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.unassigned-row {
  align-items: center;
  background: var(--surface-subtle);
  padding: var(--space-3) var(--space-4);
}

.workspace-settings {
  border-top: 1px solid var(--border-faint);
  padding: var(--space-4);
}

.workspace-requirements {
  grid-column: 1 / -1;
}

.review-summary {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
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

  .profile-card > header,
  .assignment-picker > div {
    grid-template-columns: minmax(0, 1fr);
  }

  .profile-actions {
    justify-content: flex-start;
  }

  .assignment-profile-heading,
  .unassigned-row,
  .policy-library > header,
  .assignment-library > header {
    align-items: stretch;
    flex-direction: column;
  }

  .policy-tag-list {
    justify-content: flex-start;
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
