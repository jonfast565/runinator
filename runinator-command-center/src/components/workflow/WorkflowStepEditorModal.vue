<template>
  <div
    ref="modalRoot"
    class="modal-backdrop"
    tabindex="-1"
    @keydown.esc.stop.prevent="workflows.closeStepEditor"
  >
    <form class="modal step-modal" @submit.prevent="workflows.submitStepEditor">
      <header class="modal-header">
        <div>
          <h2>{{ workflows.stepEditorCreating ? "Add Workflow Step" : "Edit Workflow Step" }}</h2>
          <span>{{ workflows.selectedStepId || "New step" }}</span>
        </div>
        <button
          type="button"
          class="btn-close"
          aria-label="Close"
          @click="workflows.closeStepEditor"
        >
          <Icon name="close" :size="16" />
        </button>
      </header>

      <section class="form-section">
        <h3>Step</h3>
        <div class="form-grid">
          <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
          <label
            >Name
            <input
              v-model="workflows.stepEditor.name"
              placeholder="Shown on the node; defaults to the step ID"
          /></label>
          <label>
            Node Kind
            <select
              v-model="workflows.stepEditor.kind"
              :disabled="workflows.selectedStepKindLocked"
            >
              <option value="start">start</option>
              <option v-for="kind in workflows.workflowNodeKinds" :key="kind" :value="kind">
                {{ workflowNodeKindLabel(kind) }}
              </option>
              <option value="end">end</option>
              <option value="fail">fail</option>
            </select>
          </label>
        </div>
      </section>

      <section class="form-section">
        <h3>Runtime</h3>
        <div class="form-grid runtime-grid">
          <label class="checkbox">
            <input
              v-model="workflows.stepEditor.locked"
              type="checkbox"
              :disabled="isProtectedNode"
            />
            Locked
          </label>
          <label class="checkbox">
            <input v-model="workflows.stepEditor.skipped" type="checkbox" />
            Skipped
          </label>
          <label
            >Max Attempts
            <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1"
          /></label>
          <label
            >Timeout Seconds
            <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0"
          /></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'action'" class="form-section">
        <div class="section-title-row">
          <h3>Action Configuration</h3>
        </div>
        <div class="form-grid">
          <label>
            Action Name
            <select :value="workflows.stepEditor.action_name" @change="onActionNameChange">
              <option value="" disabled>Select action name</option>
              <option v-if="selectedProviderMissing" :value="workflows.stepEditor.action_name">
                {{ workflows.stepEditor.action_name }} (unavailable)
              </option>
              <option
                v-for="provider in providersStore.providers"
                :key="provider.name"
                :value="provider.name"
              >
                {{ provider.name }}
              </option>
            </select>
          </label>
          <label>
            Action Function
            <select
              v-model="workflows.stepEditor.action_function"
              :disabled="!currentProvider"
              @change="applyParameterDefaults"
            >
              <option value="" disabled>
                {{ currentProvider ? "Select action function" : "Select action name first" }}
              </option>
              <option v-if="selectedActionMissing" :value="workflows.stepEditor.action_function">
                {{ workflows.stepEditor.action_function }} (unavailable)
              </option>
              <option
                v-for="action in currentActions"
                :key="action.function_name"
                :value="action.function_name"
              >
                {{ action.function_name }}
              </option>
            </select>
          </label>
        </div>
        <p v-if="selectedAction?.results?.length" class="result-metadata">
          Results:
          <span v-for="result in selectedAction.results" :key="result.name"
            >{{ result.name }} ({{ result.ty?.type ?? "any" }})</span
          >
        </p>
      </section>

      <section v-if="workflows.stepEditor.kind === 'action'" class="form-section">
        <h3>Parameters</h3>
        <TypedParameterEditor
          v-if="selectedAction && selectedAction.parameters?.length"
          v-model="stepParameters"
          :parameters="selectedAction.parameters ?? []"
          :credential-scopes="currentProvider?.metadata.credential_scopes ?? []"
          :expression-context="expressionContext"
        />
        <KeyValueObjectEditor
          v-else
          v-model="stepParameters"
          title="Action Parameters"
          empty-label="No action parameters configured."
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'approval'" class="form-section">
        <h3>Approval</h3>
        <div class="form-grid">
          <label>Approval Type <input v-model="workflows.stepEditor.approval_type" /></label>
          <label>Prompt <textarea v-model="workflows.stepEditor.approval_prompt"></textarea></label>
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'gate'" class="form-section">
        <h3>Gate</h3>
        <div class="form-grid">
          <label>
            Kind
            <select v-model="workflows.stepEditor.gate_kind">
              <option value="manual">manual</option>
              <option value="condition">condition</option>
              <option value="external">external</option>
            </select>
          </label>
          <label
            >Label
            <input v-model="workflows.stepEditor.gate_label" placeholder="Shown in the Gates view"
          /></label>
          <label
            >Poll Interval (seconds)
            <input v-model.number="workflows.stepEditor.gate_poll_interval" type="number" min="1"
          /></label>
          <label
            >Timeout (seconds, 0 = none)
            <input v-model.number="workflows.stepEditor.gate_timeout" type="number" min="0"
          /></label>
        </div>
        <p class="hint">{{ gateKindHint }}</p>
        <div v-if="workflows.stepEditor.gate_kind === 'condition'" class="form-field">
          <span class="form-field-label">When (passes once true)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.gate_when_json"
            :context="expressionContext"
            title="Gate condition"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'signal'" class="form-section">
        <h3>Signal</h3>
        <div class="form-grid">
          <label
            >Signal Name
            <input
              v-model="workflows.stepEditor.signal_name"
              placeholder="Name delivered to POST /workflow_runs/{id}/signals"
          /></label>
        </div>
        <p class="hint">
          Pauses the run until this named signal is delivered. Set a node timeout to bound the wait.
        </p>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'condition'" class="form-section">
        <h3>Condition Branches</h3>
        <div
          v-for="(branch, index) in workflows.stepEditor.condition_branches"
          :key="index"
          class="condition-branch-row"
        >
          <div class="form-field">
            <span class="form-field-label">When</span>
            <ExpressionJsonEditor
              v-model="branch.when_json"
              :context="expressionContext"
              title="Condition branch"
            />
          </div>
          <label>
            Target
            <select v-model="branch.target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button type="button" @click="workflows.removeConditionBranchEditor(index)">
            Remove
          </button>
        </div>
        <button type="button" @click="workflows.addConditionBranchEditor">Add Branch</button>
        <label>
          Fallback
          <select v-model="workflows.stepEditor.condition_fallback">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
              {{ node.id }}
            </option>
          </select>
        </label>
      </section>

      <section v-if="workflows.stepEditor.kind === 'wait'" class="form-section">
        <h3>Wait</h3>
        <div class="form-grid">
          <label
            >Seconds
            <input v-model.number="workflows.stepEditor.wait_seconds" type="number" min="0"
          /></label>
          <label>Initial Status <input v-model="workflows.stepEditor.wait_initial_status" /></label>
          <label>Until Status <input v-model="workflows.stepEditor.wait_until_status" /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Advanced Wait Settings</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.wait_json"
            :context="expressionContext"
            title="Wait settings"
          />
        </div>
      </section>

      <section v-if="workflows.stepEditor.kind === 'loop'" class="form-section">
        <h3>Loop</h3>
        <div class="form-grid">
          <label>
            Target
            <select v-model="workflows.stepEditor.loop_target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <label
            >Max Iterations
            <input v-model.number="workflows.stepEditor.loop_max_iterations" type="number" min="1"
          /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Items</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.loop_items_json"
            :context="expressionContext"
            title="Loop items"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'switch'" class="form-section">
        <h3>Switch</h3>
        <div class="form-field">
          <span class="form-field-label">Value</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.switch_value_json"
            :context="expressionContext"
            title="Switch value"
          />
        </div>
        <div
          v-for="(switchCase, index) in workflows.stepEditor.switch_cases"
          :key="index"
          class="condition-branch-row"
        >
          <label>
            Match
            <select v-model="switchCase.match_kind">
              <option value="equals">equals</option>
              <option value="not_equals">not_equals</option>
              <option value="exists">exists</option>
              <option value="when">when</option>
            </select>
          </label>
          <div class="form-field">
            <span class="form-field-label">Value</span>
            <ExpressionJsonEditor
              v-model="switchCase.match_json"
              :context="expressionContext"
              title="Switch case match"
            />
          </div>
          <label>
            Target
            <select v-model="switchCase.target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button type="button" @click="workflows.removeSwitchCaseEditor(index)">Remove</button>
        </div>
        <button type="button" @click="workflows.addSwitchCaseEditor">Add Case</button>
        <label>
          Default
          <select v-model="workflows.stepEditor.switch_default">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
              {{ node.id }}
            </option>
          </select>
        </label>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'toggle'" class="form-section">
        <h3>Toggle</h3>
        <p class="form-hint">
          A light switch: routes to <strong>on</strong> when the value is truthy, otherwise
          <strong>off</strong>.
        </p>
        <div class="form-field">
          <span class="form-field-label">Value</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.toggle_value_json"
            :context="expressionContext"
            title="Toggle value"
          />
        </div>
        <label>
          On (truthy)
          <select v-model="workflows.stepEditor.toggle_on">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
              {{ node.id }}
            </option>
          </select>
        </label>
        <label>
          Off (falsy)
          <select v-model="workflows.stepEditor.toggle_off">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
              {{ node.id }}
            </option>
          </select>
        </label>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'percentage'" class="form-section">
        <h3>Percentage</h3>
        <p class="form-hint">
          Weighted rollout, deterministic and sticky per key: <code>hash(key) % total</code> picks a
          bucket.
        </p>
        <div class="form-field">
          <span class="form-field-label">Key</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.percentage_key_json"
            :context="expressionContext"
            title="Percentage key"
          />
        </div>
        <div
          v-for="(bucket, index) in workflows.stepEditor.percentage_buckets"
          :key="index"
          class="condition-branch-row"
        >
          <label>
            Weight
            <input v-model.number="bucket.weight" type="number" min="1" />
          </label>
          <span class="form-field-label">{{ bucketShare(index) }}</span>
          <label>
            Target
            <select v-model="bucket.target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button type="button" @click="workflows.removePercentageBucketEditor(index)">
            Remove
          </button>
        </div>
        <button type="button" @click="workflows.addPercentageBucketEditor">Add Bucket</button>
        <label>
          Default (no match)
          <select v-model="workflows.stepEditor.percentage_default">
            <option value="">(none)</option>
            <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
              {{ node.id }}
            </option>
          </select>
        </label>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'parallel'" class="form-section">
        <h3>Parallel</h3>
        <div
          v-for="(_, index) in workflows.stepEditor.parallel_branches"
          :key="index"
          class="condition-branch-row"
        >
          <label>
            Branch
            <select v-model="workflows.stepEditor.parallel_branches[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button
            type="button"
            @click="workflows.removeNodeRefEditor(workflows.stepEditor.parallel_branches, index)"
          >
            Remove
          </button>
        </div>
        <button
          type="button"
          @click="workflows.addNodeRefEditor(workflows.stepEditor.parallel_branches)"
        >
          Add Branch
        </button>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'join'" class="form-section">
        <h3>Join</h3>
        <label>
          Mode
          <select v-model="workflows.stepEditor.join_mode">
            <option v-for="policy in branchPolicies" :key="policy" :value="policy">
              {{ policy }}
            </option>
          </select>
        </label>
        <div
          v-for="(_, index) in workflows.stepEditor.join_wait_for"
          :key="index"
          class="condition-branch-row"
        >
          <label>
            Wait For
            <select v-model="workflows.stepEditor.join_wait_for[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button
            type="button"
            @click="workflows.removeNodeRefEditor(workflows.stepEditor.join_wait_for, index)"
          >
            Remove
          </button>
        </div>
        <button
          type="button"
          @click="workflows.addNodeRefEditor(workflows.stepEditor.join_wait_for)"
        >
          Add Dependency
        </button>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'try'" class="form-section">
        <h3>Try</h3>
        <div class="form-grid">
          <label>
            Body
            <select v-model="workflows.stepEditor.try_body">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <label>
            Catch
            <select v-model="workflows.stepEditor.try_catch">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <label>
            Finally
            <select v-model="workflows.stepEditor.try_finally">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'map'" class="form-section">
        <h3>Map</h3>
        <div class="form-grid">
          <label>
            Target
            <select v-model="workflows.stepEditor.map_target">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <label
            >Concurrency
            <input v-model.number="workflows.stepEditor.map_concurrency" type="number" min="1"
          /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Items</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.map_items_json"
            :context="expressionContext"
            title="Map items"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'race'" class="form-section">
        <h3>Race</h3>
        <label>
          Winner
          <select v-model="workflows.stepEditor.race_winner">
            <option v-for="policy in branchPolicies" :key="policy" :value="policy">
              {{ policy }}
            </option>
          </select>
        </label>
        <div
          v-for="(_, index) in workflows.stepEditor.race_branches"
          :key="index"
          class="condition-branch-row"
        >
          <label>
            Branch
            <select v-model="workflows.stepEditor.race_branches[index]">
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
          <button
            type="button"
            @click="workflows.removeNodeRefEditor(workflows.stepEditor.race_branches, index)"
          >
            Remove
          </button>
        </div>
        <button
          type="button"
          @click="workflows.addNodeRefEditor(workflows.stepEditor.race_branches)"
        >
          Add Branch
        </button>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'output'" class="form-section">
        <h3>Output</h3>
        <label>Event Type <input v-model="workflows.stepEditor.output_event_type" /></label>
        <div class="form-field">
          <span class="form-field-label">Data</span>
          <KeyValueObjectEditor
            v-if="outputDataIsObject"
            v-model="outputDataObject"
            empty-label="No output fields configured."
            :expression-context="expressionContext"
          />
          <p v-else class="hint">
            This output data is a WDL expression or non-object value. Edit it from the raw WDL data
            disclosure below.
          </p>
        </div>
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.output_data_json"
          :context="expressionContext"
          title="Raw WDL data"
        />
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'input'" class="form-section">
        <h3>Input</h3>
        <label>Prompt <input v-model="workflows.stepEditor.input_prompt" /></label>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'config'" class="form-section">
        <h3>Config</h3>
        <div class="form-field">
          <span class="form-field-label">Name</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.config_name_json"
            :context="expressionContext"
            title="Config name"
          />
        </div>
        <div class="form-field">
          <span class="form-field-label">Metadata</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.config_metadata_json"
            :context="expressionContext"
            title="Config metadata"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'subflow'" class="form-section">
        <h3>Subflow</h3>
        <div class="form-grid">
          <label>
            Workflow
            <select :value="selectedSubflowName || ''" @change="onSubflowNameChange">
              <option value="">Select a workflow</option>
              <option
                v-if="selectedSubflowMissing"
                :value="String(workflows.stepEditor.subflow_id ?? '')"
              >
                {{ selectedSubflowName }} (unavailable)
              </option>
              <option
                v-for="workflow in availableSubflows"
                :key="String(workflow.id)"
                :value="workflow.name"
              >
                {{ workflow.name }}
              </option>
            </select>
          </label>
        </div>
        <h3>Parameters</h3>
        <TypedValueEditor
          v-if="selectedSubflowInputType"
          :ty="selectedSubflowInputType"
          :model-value="subflowParameters"
          :expression-context="expressionContext"
          @update:model-value="onSubflowParametersChange"
        />
        <KeyValueObjectEditor
          v-else
          v-model="subflowParameters"
          empty-label="Select a workflow or add subflow parameters."
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.subflow_parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'assert'" class="form-section">
        <h3>Assertions</h3>
        <p class="form-hint">
          Each assertion is evaluated against the run context; the node fails with a structured
          violation list if any is false.
        </p>
        <div
          v-for="(assertion, index) in workflows.stepEditor.assert_assertions"
          :key="index"
          class="assertion-row"
        >
          <div class="assertion-row-head">
            <label>Name <input v-model="assertion.name" placeholder="unnamed" /></label>
            <button type="button" @click="workflows.removeAssertionEditor(index)">Remove</button>
          </div>
          <div class="form-field">
            <span class="form-field-label">Condition (must be true)</span>
            <ExpressionJsonEditor
              v-model="assertion.condition_json"
              :context="expressionContext"
              title="Assertion condition"
            />
          </div>
          <label
            >Message <input v-model="assertion.message" placeholder="Assertion failed"
          /></label>
        </div>
        <button type="button" @click="workflows.addAssertionEditor">Add Assertion</button>
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'transform'" class="form-section">
        <h3>Transform</h3>
        <p class="form-hint">
          Each binding resolves an expression into the run context under its name. No side effects.
        </p>
        <KeyValueObjectEditor
          v-model="transformBindings"
          title="Bindings"
          empty-label="No bindings configured."
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'audit'" class="form-section">
        <h3>Audit</h3>
        <p class="form-hint">Appends a tamper-evident audit record to the workflow log.</p>
        <div class="form-field">
          <span class="form-field-label">Action</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.audit_action_json"
            :context="expressionContext"
            title="Audit action"
          />
        </div>
        <div class="form-field">
          <span class="form-field-label">Actor (optional)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.audit_actor_json"
            :context="expressionContext"
            title="Audit actor"
          />
        </div>
        <div class="form-field">
          <span class="form-field-label">Target (optional)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.audit_target_json"
            :context="expressionContext"
            title="Audit target"
          />
        </div>
        <div class="form-field">
          <span class="form-field-label">Reason (optional)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.audit_reason_json"
            :context="expressionContext"
            title="Audit reason"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Metadata &amp; Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'checkpoint'" class="form-section">
        <h3>Checkpoint</h3>
        <p class="form-hint">
          Snapshots run state at a named point; enables rollback via the control-plane API.
        </p>
        <div class="form-grid">
          <label
            >Name
            <input v-model="workflows.stepEditor.checkpoint_name" placeholder="checkpoint name"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'mutex'" class="form-section">
        <h3>Mutex</h3>
        <p class="form-hint">
          Acquires a named distributed lock; parks until it is available. Set a node timeout to
          bound the wait.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.mutex_name" placeholder="lock name"
          /></label>
          <label
            >Poll Interval (seconds)
            <input v-model.number="workflows.stepEditor.mutex_poll_interval" type="number" min="1"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'throttle'" class="form-section">
        <h3>Throttle</h3>
        <p class="form-hint">
          Enforces a cross-run rate limit; parks until a token is available in the rolling window.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.throttle_name" placeholder="limiter name"
          /></label>
          <label
            >Max Per Window
            <input
              v-model.number="workflows.stepEditor.throttle_max_per_window"
              type="number"
              min="1"
          /></label>
          <label
            >Window (seconds)
            <input
              v-model.number="workflows.stepEditor.throttle_window_seconds"
              type="number"
              min="1"
          /></label>
          <label
            >Poll Interval (seconds)
            <input
              v-model.number="workflows.stepEditor.throttle_poll_interval"
              type="number"
              min="1"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'await_run'" class="form-section">
        <h3>Await Run</h3>
        <p class="form-hint">
          Waits for one or more independently-started runs to reach a terminal state.
        </p>
        <div class="form-field">
          <span class="form-field-label">Run IDs (array of UUIDs or an expression)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.await_run_ids_json"
            :context="expressionContext"
            title="Await run ids"
          />
        </div>
        <div class="form-grid">
          <label>
            Mode
            <select v-model="workflows.stepEditor.await_mode">
              <option value="all">all</option>
              <option value="any">any</option>
            </select>
          </label>
          <label
            >Poll Interval (seconds)
            <input v-model.number="workflows.stepEditor.await_poll_interval" type="number" min="1"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'debounce'" class="form-section">
        <h3>Debounce</h3>
        <p class="form-hint">
          Parks for a trailing delay that resets when re-triggered; collapses event bursts.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.debounce_name" placeholder="debounce name"
          /></label>
          <label
            >Delay (seconds)
            <input
              v-model.number="workflows.stepEditor.debounce_delay_seconds"
              type="number"
              min="1"
          /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Trigger Key (optional)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.debounce_trigger_key_json"
            :context="expressionContext"
            title="Debounce trigger key"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'collect'" class="form-section">
        <h3>Collect</h3>
        <p class="form-hint">
          Accumulates externally-delivered items until the count threshold is met. Set a node
          timeout to bound the wait.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.collect_name" placeholder="collector name"
          /></label>
          <label
            >Max Items
            <input v-model.number="workflows.stepEditor.collect_max" type="number" min="1"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'barrier'" class="form-section">
        <h3>Barrier</h3>
        <p class="form-hint">
          Parks until N runs reach this named barrier; the last arrival releases all waiters.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.barrier_name" placeholder="barrier name"
          /></label>
          <label
            >Count <input v-model.number="workflows.stepEditor.barrier_count" type="number" min="1"
          /></label>
          <label
            >Poll Interval (seconds)
            <input
              v-model.number="workflows.stepEditor.barrier_poll_interval"
              type="number"
              min="1"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'circuit_breaker'" class="form-section">
        <h3>Circuit Breaker</h3>
        <p class="form-hint">
          Tracks failure rates across runs; fast-fails via <code>on_failure</code> when tripped,
          then recovers after the cooldown.
        </p>
        <div class="form-grid">
          <label
            >Name <input v-model="workflows.stepEditor.circuit_name" placeholder="breaker name"
          /></label>
          <label
            >Threshold (failures)
            <input v-model.number="workflows.stepEditor.circuit_threshold" type="number" min="1"
          /></label>
          <label
            >Window (seconds)
            <input
              v-model.number="workflows.stepEditor.circuit_window_seconds"
              type="number"
              min="1"
          /></label>
          <label
            >Cooldown (seconds)
            <input
              v-model.number="workflows.stepEditor.circuit_cooldown_seconds"
              type="number"
              min="0"
          /></label>
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section v-if="workflows.stepEditor.kind === 'event_source'" class="form-section">
        <h3>Event Source</h3>
        <p class="form-hint">
          Subscribes to a named event stream and drives the body subgraph on each matching event.
          Use <code>*</code> to match any type.
        </p>
        <div class="form-grid">
          <label
            >Event Type <input v-model="workflows.stepEditor.event_source_type" placeholder="*"
          /></label>
          <label
            >Max Events (0 = unlimited)
            <input v-model.number="workflows.stepEditor.event_source_max" type="number" min="0"
          /></label>
        </div>
        <div class="form-field">
          <span class="form-field-label">Filter (optional)</span>
          <ExpressionJsonEditor
            v-model="workflows.stepEditor.event_source_filter_json"
            :context="expressionContext"
            title="Event source filter"
          />
        </div>
        <KeyValueObjectEditor
          v-model="additionalParameters"
          title="Additional Parameters"
          :expression-context="expressionContext"
        />
        <AdvancedWdlParameters
          v-model="workflows.stepEditor.parameters_json"
          :context="expressionContext"
          title="Raw WDL parameters"
        />
      </section>

      <section class="form-section">
        <h3>Transitions</h3>
        <div class="transition-grid">
          <label v-for="key in workflows.directTransitionKeys" :key="key">
            {{ key }}
            <select
              :value="workflows.getTransition(key)"
              @change="workflows.setTransition(key, ($event.target as HTMLSelectElement).value)"
            >
              <option value="">(none)</option>
              <option v-for="node in targetNodes" :key="String(node.id)" :value="node.id">
                {{ node.id }}
              </option>
            </select>
          </label>
        </div>
      </section>

      <section v-if="referenceGroups.length" class="form-section">
        <h3>Available References</h3>
        <ReferenceChips :groups="referenceGroups" />
      </section>

      <p v-if="workflows.stepEditorError" class="error">{{ workflows.stepEditorError }}</p>
      <div class="modal-actions">
        <button type="button" class="btn" @click="workflows.closeStepEditor">Cancel</button>
        <button type="submit" class="btn btn-primary">Apply Step</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useProvidersStore } from "../../stores/providers";
import { buildInputSkeleton, useWorkflowsStore } from "../../stores/workflows";
import { pretty } from "../../utils/format";
import { parseObject } from "../../utils/json";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import AdvancedWdlParameters from "../shared/AdvancedWdlParameters.vue";
import KeyValueObjectEditor from "../shared/KeyValueObjectEditor.vue";
import ReferenceChips from "../shared/ReferenceChips.vue";
import { buildSampleContext, workflowReferenceGroups } from "../../utils/workflow-references";
import { asArray, isRecord, workflowNodeKindLabel } from "../../utils/workflows";
import { displayValue } from "../../utils/values";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";
import TypedValueEditor from "../shared/TypedValueEditor.vue";
import Icon from "../shared/Icon.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const branchPolicies = ["all", "any", "first_success"] as const;

const currentProvider = computed(
  () =>
    providersStore.providers.find(
      (provider) => provider.name === workflows.stepEditor.action_name,
    ) ?? null,
);
const currentActions = computed(() => currentProvider.value?.actions ?? []);
const selectedAction = computed(
  () =>
    currentActions.value.find(
      (action) => action.function_name === workflows.stepEditor.action_function,
    ) ?? null,
);
const selectedProviderMissing = computed(() =>
  Boolean(workflows.stepEditor.action_name && !currentProvider.value),
);
const selectedActionMissing = computed(() =>
  Boolean(
    workflows.stepEditor.action_function &&
    currentProvider.value &&
    !currentActions.value.some(
      (action) => action.function_name === workflows.stepEditor.action_function,
    ),
  ),
);
const stepParameters = computed({
  get: () => parseObject(workflows.stepEditor.parameters_json, {}),
  set: (value) => {
    workflows.stepEditor.parameters_json = pretty(value);
  },
});
const reservedParameterKeys = computed(() => {
  switch (workflows.stepEditor.kind) {
    case "approval":
      return new Set(["approval_type", "prompt"]);
    case "gate":
      return new Set(["kind", "when", "poll_interval", "timeout", "label"]);
    case "signal":
      return new Set(["name"]);
    case "loop":
      return new Set(["items", "target"]);
    case "switch":
      return new Set(["value", "cases", "default"]);
    case "parallel":
      return new Set(["branches"]);
    case "join":
      return new Set(["wait_for", "mode"]);
    case "try":
      return new Set(["body", "catch", "finally"]);
    case "map":
      return new Set(["items", "target", "concurrency"]);
    case "race":
      return new Set(["branches", "winner"]);
    case "output":
      return new Set(["event_type", "data"]);
    case "input":
      return new Set(["prompt"]);
    case "config":
      return new Set(["name", "metadata"]);
    case "assert":
      return new Set(["assertions"]);
    case "transform":
      return new Set(["bindings"]);
    case "audit":
      return new Set(["action", "actor", "target", "reason"]);
    case "checkpoint":
      return new Set(["name"]);
    case "mutex":
      return new Set(["name", "poll_interval_seconds"]);
    case "throttle":
      return new Set(["name", "max_per_window", "window_seconds", "poll_interval_seconds"]);
    case "await_run":
      return new Set(["run_ids", "mode", "poll_interval_seconds"]);
    case "debounce":
      return new Set(["name", "delay_seconds", "trigger_key"]);
    case "collect":
      return new Set(["name", "max"]);
    case "barrier":
      return new Set(["name", "count", "poll_interval_seconds"]);
    case "circuit_breaker":
      return new Set(["name", "threshold", "window_seconds", "cooldown_seconds"]);
    case "event_source":
      return new Set(["event_type", "filter", "max"]);
    default:
      return new Set<string>();
  }
});
const additionalParameters = computed({
  get: () => omitKeys(stepParameters.value, reservedParameterKeys.value),
  set: (value) => {
    const reserved = pickKeys(stepParameters.value, reservedParameterKeys.value);
    workflows.stepEditor.parameters_json = pretty({ ...reserved, ...value });
  },
});
const isProtectedNode = computed(() =>
  ["start", "end", "fail"].includes(displayValue(workflows.selectedNode?.kind ?? "")),
);
const gateKindHint = computed(() => {
  switch (workflows.stepEditor.gate_kind) {
    case "condition":
      return "The reducer auto-evaluates the condition each poll; the gate passes once it is true.";
    case "external":
      return "Stays closed until an external system opens it via POST /gates/{id}/open.";
    default:
      return "Stays closed until a human opens it from the Gates view.";
  }
});
const targetNodes = computed(() => {
  const nodes = asArray(workflows.workflowDraft.definition.nodes).filter(isRecord);
  return nodes.filter((node) => node.id !== workflows.selectedStepId);
});

// the effective traffic share of a percentage bucket = its weight over the total of all weights.
function bucketShare(index: number): string {
  const buckets = workflows.stepEditor.percentage_buckets;
  const total = buckets.reduce((sum, bucket) => sum + (bucket.weight || 0), 0);
  const weight = buckets[index]?.weight || 0;

  if (total <= 0) {
    return "—";
  }

  return `${String(Math.round((weight / total) * 100))}%`;
}

const expressionContext = computed(() => ({
  workflowInputType: workflows.workflowDraft.input_type,
  nodes: asArray(workflows.workflowDraft.definition.nodes).filter(isRecord),
  currentNodeId: workflows.selectedStepId,
  providers: providersStore.providers,
  // a loaded run's data lets the editor preview resolved values against real outputs.
  sampleContext: buildSampleContext(workflows.workflowRunDetail),
}));

// the references in scope at this node (params, prior node outputs, run roots) for the chip list.
const referenceGroups = computed(() => workflowReferenceGroups(expressionContext.value));

const availableSubflows = computed(() => {
  const currentId = workflows.selectedWorkflowId;
  return workflows.workflows.filter((w) => w.id !== currentId);
});

const selectedSubflowName = computed(() => {
  if (!workflows.stepEditor.subflow_id) {
    return "";
  }

  const workflow = workflows.workflows.find((w) => w.id === workflows.stepEditor.subflow_id);
  return workflow?.name ?? "";
});

const selectedSubflowMissing = computed(() => {
  return Boolean(workflows.stepEditor.subflow_id && !selectedSubflowName.value);
});

// the child workflow's declared input schema drives the typed parameter form.
const selectedSubflowInputType = computed(() => {
  const workflow = workflows.workflows.find((w) => w.id === workflows.stepEditor.subflow_id);
  return workflow?.input_type ?? null;
});

const subflowParameters = computed({
  get: () => parseObject(workflows.stepEditor.subflow_parameters_json, {}),
  set: (value) => {
    workflows.stepEditor.subflow_parameters_json = pretty(value);
  },
});

const outputDataObject = computed({
  get: () => parseObject(workflows.stepEditor.output_data_json, {}),
  set: (value) => {
    workflows.stepEditor.output_data_json = pretty(value);
  },
});
const transformBindings = computed({
  get: () => parseObject(workflows.stepEditor.transform_bindings_json, {}),
  set: (value) => {
    workflows.stepEditor.transform_bindings_json = pretty(value);
  },
});
const outputDataIsObject = computed(() => {
  try {
    const value: unknown = JSON.parse(workflows.stepEditor.output_data_json || "{}");
    return Boolean(value && typeof value === "object" && !Array.isArray(value));
  } catch {
    return false;
  }
});

// the typed editor and the raw-json fallback both write back to the same json string.
function onSubflowParametersChange(value: unknown) {
  const object = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  workflows.stepEditor.subflow_parameters_json = pretty(object);
}

function omitKeys(value: Record<string, unknown>, keys: Set<string>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(value).filter(([key]) => !keys.has(key)));
}

function pickKeys(value: Record<string, unknown>, keys: Set<string>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(value).filter(([key]) => keys.has(key)));
}

// the modal owns its escape handling via a scoped @keydown on its root, so focus it on open.
const modalRoot = ref<HTMLElement | null>(null);

onMounted(() => {
  if (providersStore.providers.length === 0 && !providersStore.loading) {
    void providersStore.fetchProviders();
  }

  modalRoot.value?.focus();
});

function onActionNameChange(event: Event) {
  const name = (event.target as HTMLSelectElement).value;
  workflows.stepEditor.action_name = name;
  const provider = providersStore.providers.find((item) => item.name === name);
  workflows.stepEditor.action_function = provider?.actions[0]?.function_name ?? "";
  applyParameterDefaults();
}

function applyParameterDefaults() {
  if (!selectedAction.value) {
    return;
  }

  const next = { ...stepParameters.value };

  for (const parameter of selectedAction.value.parameters) {
    if (next[parameter.name] === undefined && parameter.default_value !== undefined) {
      next[parameter.name] = parameter.default_value;
    }
  }

  stepParameters.value = next;
}

function onSubflowNameChange(event: Event) {
  const name = (event.target as HTMLSelectElement).value;
  const workflow = workflows.workflows.find((w) => w.name === name);

  if (!workflow?.id) {
    return;
  }

  workflows.stepEditor.subflow_id = workflow.id;

  // seed declared fields when no parameters are set yet, so the form renders pre-populated.
  if (Object.keys(subflowParameters.value).length === 0) {
    onSubflowParametersChange(buildInputSkeleton(workflow.input_type));
  }
}
</script>

<style scoped>
.step-modal {
  width: min(1040px, 100%);
}

.modal-header span,
.result-metadata {
  color: var(--text-muted);
  font-size: 12px;
}

.hint {
  color: var(--text-muted);
  font-size: 12px;
}

.section-title-row {
  display: flex;
  align-items: end;
  justify-content: space-between;
  gap: 12px;
}

.section-title-row label {
  min-width: 260px;
}

.transition-grid {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(5, minmax(0, 1fr));
}

.condition-branch-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 180px auto;
  gap: 8px;
  align-items: end;
}

.assertion-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  margin-bottom: 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
}

.assertion-row-head {
  display: flex;
  gap: 8px;
  align-items: end;
  justify-content: space-between;
}

.assertion-row-head label {
  flex: 1;
}

.result-metadata {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
</style>
