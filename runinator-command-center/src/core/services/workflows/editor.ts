import {
  cancelWorkflowRun, closeGate, compileWdl, continueWorkflowRun, createWorkflowRun, decompileToWdl,
  deleteWorkflow, deleteWorkflowTrigger, duplicateWorkflow, fetchGates, fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks, fetchWorkflowRun, fetchWorkflowRuns, fetchWorkflowTriggers, fetchWorkflows,
  openGate, patchWorkflowRunDebug, pauseWorkflowRun, renameWorkflowRun as renameWorkflowRunApi,
  replayWorkflowRun as replayWorkflowRunApi, resumeWorkflowRun, rerunWorkflowNode, runToCursorWorkflowRun,
  saveWorkflowWdl, saveWorkflowTrigger, skipWorkflowNode, stepWorkflowRun,
  type WorkflowDebugPatch, type WorkflowWdlSaveRequest,
} from "../../api/commandCenterApi";
import type {
  GateRecord, JsonRecord, JsonValue, ProviderMetadata, RunArtifact, RunChunk, RunSummary,
  RuninatorType, WorkflowDefinition, WorkflowEdgeEditorDraft, WorkflowEditorEdgeData,
  WorkflowLayoutDirection, WorkflowNodeKind, WorkflowNodeRun, WorkflowRunDetail, WorkflowTrigger,
  WorkflowTriggerKind, WorkflowValidationIssue,
} from "../../domain/models";
import { asJsonValue } from "../../domain/json";
import { pretty } from "../../utils/format";
import { cloneJson, parseObject, parseRequiredJson, parseRequiredObject } from "../../utils/json";
import { displayValue, isBlankValue } from "../../utils/values";
import { createZip, type ZipEntry } from "../../utils/zip";
import {
  applyWorkflowEdgeEditorDraft, applyWorkflowInlineNodeEdit, asArray, asRecord, isRecord,
  autoArrangeWorkflowEdgeHandles, autoArrangeWorkflowLayout, createWorkflowNode, directTransitionKeys,
  isSameConnectionPointLoop, nodeRef, nodeRefId, normalizeWorkflowDefinition, parameterSemanticKey,
  removeConditionBranch, removeWorkflowEdge, removeWorkflowEdgeHandles, removeWorkflowNodeReferences,
  setConditionBranch, setWorkflowEdgeHandles, setWorkflowEdgeLabelAnchor, setWorkflowEdgeLabelOffset,
  moveWorkflowEdgeEditorDraft, optionIdForSourceHandle, workflowEdgeOptionId, workflowEdgeEditorDraft,
  workflowEdgeSemanticOptions, uniqueWorkflowNodeId, validateWorkflowReferenceSyntax, valueRef,
  workflowNodeActionConfig, workflowNodeActionInputs,
} from "../../workflow/index";
import type { GraphEdgeLike, GraphEdgeModel } from "../../workflow/graph-model";
import {
  branchPolicyName, boundedIndex, defaultEdgeEditorDraft, defaultTriggerConfiguration, errorMessage,
  formatMaybeDate, dateTimeLocalToIso, isLockedWorkflowNode, isProtectedWorkflowNode, newWorkflowDraft,
  newWorkflowTriggerDraft, nextNodePosition, nodeRefArray, switchCaseEditor, validateJsonValueType,
} from "../../workflow/editor-defaults";
import type { WorkflowServiceHost } from "./host";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;
const MAX_OPEN_RUN_TABS = 8;
const WATCH_STORAGE_PREFIX = "runinator.watch.";

export type WorkflowCatalogPeer = {
  saveSelectedWorkflowBundle: () => Promise<void>;
};

export type WorkflowRunsPeer = {
  clearWorkflowRunGates: () => void;
  updateSelectedWorkflowNodeDetail: () => Promise<void>;
};

export function createWorkflowEditorService(
  host: WorkflowServiceHost,
  runs: WorkflowRunsPeer,
  catalog: WorkflowCatalogPeer,
) {
  const { deps, internal } = host;

  function addWorkflowStep() {
    addWorkflowNode("action");
  }

  function addWorkflowNode(kind: WorkflowNodeKind) {
    const nodes = ensureWorkflowNodes();
    const newNode = createWorkflowNode(kind, nodes);
    stripNewNodeConnections(newNode);
    const position = graphCentroidPosition();
    const endIndex = nodes.findIndex((node: JsonRecord) => node.kind === "end");

    if (endIndex >= 0) {
      nodes.splice(endIndex, 0, newNode);
    } else {
      nodes.push(newNode);
    }

    setGraphNodePosition(displayValue(newNode.id), position);
    syncWorkflowDraftToJson();
    populateStepEditor(displayValue(newNode.id));
    openStepEditor(displayValue(newNode.id), true);
  }

  function addConnectedWorkflowNode(kind: WorkflowNodeKind = "action") {
    addWorkflowNode(kind);
  }

  function removeWorkflowStep() {
    if (!host.state.selectedStepId || !host.canRemoveSelectedStep()) {
      return;
    }

    removeWorkflowNode(host.state.selectedStepId);
  }

  function removeWorkflowNode(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node || isLockedWorkflowNode(node)) {
      return;
    }

    host.state.workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
      (item: JsonRecord) => item.id !== nodeId,
    );
    removeWorkflowNodeReferences(host.state.workflowDraft.definition, nodeId);
    const layout = asRecord(asRecord(host.state.workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = Object.fromEntries(
      Object.entries(layoutNodes).filter(([entryId]) => entryId !== nodeId),
    );

    if (host.state.selectedStepId === nodeId) {
      host.state.selectedStepId = "";
    }

    syncWorkflowDraftToJson();
  }

  function applyInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    const previousId = nodeId;
    const result = applyWorkflowInlineNodeEdit(
      host.state.workflowDraft.definition,
      nodeId,
      nextId,
      inlineValue,
    );

    if (!result.ok) {
      host.ctx.setError(result.message);
      return false;
    }

    if (previousId !== result.nodeId) {
      renameLayoutNode(previousId, result.nodeId);
    }

    host.state.selectedStepId = result.nodeId;
    syncWorkflowDraftToJson();
    populateStepEditor(result.nodeId);
    return true;
  }

  function clearWorkflowGraphSelection() {
    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    host.state.selectedGraphEdgeId = "";
  }

  function submitInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    if (!applyInlineNodeEdit(nodeId, nextId, inlineValue)) {
      return false;
    }

    clearWorkflowGraphSelection();
    return true;
  }

  function applyStepEditor(): boolean {
    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    host.state.stepEditorError = "";

    if (!host.state.selectedStepId) {
      return false;
    }

    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === host.state.selectedStepId);

    if (index < 0) {
      return false;
    }

    if (isLockedWorkflowNode(nodes[index]) && host.state.stepEditor.kind !== nodes[index].kind) {
      const message = `${String(nodes[index].kind)} node kind cannot be changed`;
      host.state.stepEditorError = message;
      host.ctx.setError(message);
      return false;
    }

    const parameters = parseRequiredObject(host.state.stepEditor.parameters_json);
    const transitions = parseRequiredObject(host.state.stepEditor.transitions_json);

    if (!parameters || !transitions) {
      const message = parameters
        ? "Node transitions must be a JSON object"
        : "Step parameters must be a JSON object";
      host.state.stepEditorError = message;
      host.ctx.setError(message);
      return false;
    }

    const parameterError = validateStepParameters(parameters);

    if (parameterError) {
      host.state.stepEditorError = parameterError;
      host.ctx.setError(parameterError);
      return false;
    }

    // named slots so flow analysis tracks parameters/transitions/action edits
    // that a bare JsonRecord index signature would keep as `unknown`.
    type EditableNode = JsonRecord & {
      id?: string;
      action?: JsonRecord;
      parameters?: JsonRecord;
      transitions?: JsonRecord;
      wait?: JsonRecord;
      retry?: JsonRecord;
    };
    const next = { ...nodes[index] } as EditableNode;
    next.id = host.state.stepEditor.id.trim();

    if (!next.id) {
      host.state.stepEditorError = "Step ID is required";
      return false;
    }

    const trimmedName = host.state.stepEditor.name.trim();

    if (trimmedName) {
      next.name = trimmedName;
    } else {
      delete next.name;
    }

    next.kind = host.state.stepEditor.kind;

    if (next.kind === "action") {
      const previousAction = isRecord(next.action) ? next.action : {};
      next.action = {
        ...previousAction,
        provider: host.state.stepEditor.action_name,
        function: host.state.stepEditor.action_function,
        timeout_seconds:
          host.state.stepEditor.timeout_seconds > 0
            ? host.state.stepEditor.timeout_seconds
            : (previousAction.timeout_seconds ?? 300),
        configuration: parameters,
      };
    } else {
      delete next.action;
    }

    next.retry = { max_attempts: host.state.stepEditor.max_attempts };

    if (host.state.stepEditor.timeout_seconds > 0) {
      next.timeout_seconds = host.state.stepEditor.timeout_seconds;
    } else {
      delete next.timeout_seconds;
    }

    if (isProtectedWorkflowNode(next)) {
      delete next.locked;
    } else if (host.state.stepEditor.locked) {
      next.locked = true;
    } else {
      delete next.locked;
    }

    if (host.state.stepEditor.skipped) {
      next.skipped = true;
    } else {
      delete next.skipped;
    }

    // action nodes store inputs in action.configuration (set above); keep node.parameters clear to avoid duplication.
    next.parameters = next.kind === "action" ? {} : parameters;
    next.transitions = transitions;

    if (next.kind === "approval") {
      next.parameters = {
        ...parameters,
        approval_type: host.state.stepEditor.approval_type || "generic",
        prompt: host.state.stepEditor.approval_prompt || "Approval required",
      };
    }

    if (next.kind === "gate") {
      next.parameters = { ...parameters, kind: host.state.stepEditor.gate_kind || "manual" };

      if (host.state.stepEditor.gate_kind === "condition") {
        const when = parseRequiredObject(host.state.stepEditor.gate_when_json);

        if (!when) {
          host.state.stepEditorError = "Gate condition must be a JSON object";
          host.ctx.setError(host.state.stepEditorError);
          return false;
        }

        next.parameters.when = when;
      } else {
        delete next.parameters.when;
      }

      const pollInterval = host.state.stepEditor.gate_poll_interval;

      if (pollInterval > 0) {
        next.parameters.poll_interval = pollInterval;
      } else {
        delete next.parameters.poll_interval;
      }

      const timeout = host.state.stepEditor.gate_timeout;

      if (timeout > 0) {
        next.parameters.timeout = timeout;
      } else {
        delete next.parameters.timeout;
      }

      if (host.state.stepEditor.gate_label.trim()) {
        next.parameters.label = host.state.stepEditor.gate_label.trim();
      } else {
        delete next.parameters.label;
      }
    }

    if (next.kind === "signal") {
      next.parameters = { ...parameters, name: host.state.stepEditor.signal_name.trim() || "signal" };
    }

    if (next.kind === "condition") {
      next.transitions = { ...transitions, branches: [] };

      for (const [branchIndex, branch] of host.state.stepEditor.condition_branches.entries()) {
        const when = parseRequiredObject(branch.when_json);

        if (!when) {
          host.state.stepEditorError = `Condition branch ${String(branchIndex + 1)} must be a JSON object`;
          host.ctx.setError(host.state.stepEditorError);
          return false;
        }

        if (!branch.target) {
          host.state.stepEditorError = `Condition branch ${String(branchIndex + 1)} needs a target`;
          host.ctx.setError(host.state.stepEditorError);
          return false;
        }

        setConditionBranch(next, branchIndex, when, branch.target);
      }

      if (host.state.stepEditor.condition_fallback) {
        next.transitions.next = nodeRef(host.state.stepEditor.condition_fallback);
      } else {
        delete next.transitions.next;
      }
    }

    if (next.kind === "wait") {
      const wait = parseRequiredObject(host.state.stepEditor.wait_json);

      if (!wait) {
        host.state.stepEditorError = "Wait settings must be a JSON object";
        host.ctx.setError(host.state.stepEditorError);
        return false;
      }

      next.wait = {
        ...wait,
        seconds: Math.max(0, host.state.stepEditor.wait_seconds),
      };

      if (host.state.stepEditor.wait_initial_status.trim()) {
        next.wait.initial_status = host.state.stepEditor.wait_initial_status.trim();
      } else {
        delete next.wait.initial_status;
      }

      if (host.state.stepEditor.wait_until_status.trim()) {
        next.wait.until_status = host.state.stepEditor.wait_until_status.trim();
      } else {
        delete next.wait.until_status;
      }
    } else {
      delete next.wait;
    }

    if (next.kind === "loop") {
      const items = parseStepJson("Loop items", host.state.stepEditor.loop_items_json);

      if (!items.ok) {
        return false;
      }

      next.parameters = { ...parameters, items: items.value };

      if (host.state.stepEditor.loop_target) {
        next.parameters.target = nodeRef(host.state.stepEditor.loop_target);
      } else {
        delete next.parameters.target;
      }

      next.max_iterations = Math.max(1, host.state.stepEditor.loop_max_iterations);
    } else {
      delete next.max_iterations;
    }

    if (next.kind === "switch") {
      const value = parseStepJson("Switch value", host.state.stepEditor.switch_value_json);

      if (!value.ok) {
        return false;
      }

      const cases: JsonRecord[] = [];

      for (const [caseIndex, switchCase] of host.state.stepEditor.switch_cases.entries()) {
        if (!switchCase.target) {
          setStepEditorError(`Switch case ${String(caseIndex + 1)} needs a target`);
          return false;
        }

        const match = parseStepJson(`Switch case ${String(caseIndex + 1)}`, switchCase.match_json);

        if (!match.ok) {
          return false;
        }

        const serialized: JsonRecord = { target: nodeRef(switchCase.target) };

        if (switchCase.match_kind === "when") {
          serialized.when = match.value;
        } else if (switchCase.match_kind === "exists") {
          serialized.exists = Boolean(match.value);
        } else {
          serialized[switchCase.match_kind] = match.value;
        }

        cases.push(serialized);
      }

      next.parameters = { ...parameters, value: value.value, cases };

      if (host.state.stepEditor.switch_default) {
        next.parameters.default = nodeRef(host.state.stepEditor.switch_default);
      } else {
        delete next.parameters.default;
      }
    }

    if (next.kind === "toggle") {
      const value = parseStepJson("Toggle value", host.state.stepEditor.toggle_value_json);

      if (!value.ok) {
        return false;
      }

      if (!host.state.stepEditor.toggle_on || !host.state.stepEditor.toggle_off) {
        setStepEditorError("Toggle needs both an on and an off target");
        return false;
      }

      next.parameters = {
        ...parameters,
        value: value.value,
        on: nodeRef(host.state.stepEditor.toggle_on),
        off: nodeRef(host.state.stepEditor.toggle_off),
      };
    }

    if (next.kind === "percentage") {
      const key = parseStepJson("Percentage key", host.state.stepEditor.percentage_key_json);

      if (!key.ok) {
        return false;
      }

      const buckets: JsonRecord[] = [];

      for (const [bucketIndex, bucket] of host.state.stepEditor.percentage_buckets.entries()) {
        if (!bucket.target) {
          setStepEditorError(`Bucket ${String(bucketIndex + 1)} needs a target`);
          return false;
        }

        const weight = Math.trunc(bucket.weight);

        if (!Number.isFinite(weight) || weight <= 0) {
          setStepEditorError(`Bucket ${String(bucketIndex + 1)} needs a weight greater than zero`);
          return false;
        }

        buckets.push({ weight, target: nodeRef(bucket.target) });
      }

      next.parameters = { ...parameters, key: key.value, buckets };

      if (host.state.stepEditor.percentage_default) {
        next.parameters.default = nodeRef(host.state.stepEditor.percentage_default);
      } else {
        delete next.parameters.default;
      }
    }

    if (next.kind === "parallel") {
      next.parameters = {
        ...parameters,
        branches: host.state.stepEditor.parallel_branches.filter(Boolean).map(nodeRef),
      };
    }

    if (next.kind === "join") {
      next.parameters = {
        ...parameters,
        wait_for: host.state.stepEditor.join_wait_for.filter(Boolean).map(nodeRef),
        mode: host.state.stepEditor.join_mode,
      };
    }

    if (next.kind === "try") {
      next.parameters = { ...parameters };

      if (host.state.stepEditor.try_body) {
        next.parameters.body = nodeRef(host.state.stepEditor.try_body);
      } else {
        delete next.parameters.body;
      }

      if (host.state.stepEditor.try_catch) {
        next.parameters.catch = nodeRef(host.state.stepEditor.try_catch);
      } else {
        delete next.parameters.catch;
      }

      if (host.state.stepEditor.try_finally) {
        next.parameters.finally = nodeRef(host.state.stepEditor.try_finally);
      } else {
        delete next.parameters.finally;
      }
    }

    if (next.kind === "map") {
      const items = parseStepJson("Map items", host.state.stepEditor.map_items_json);

      if (!items.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        items: items.value,
        concurrency: Math.max(1, host.state.stepEditor.map_concurrency),
      };

      if (host.state.stepEditor.map_target) {
        next.parameters.target = nodeRef(host.state.stepEditor.map_target);
      } else {
        delete next.parameters.target;
      }
    }

    if (next.kind === "race") {
      next.parameters = {
        ...parameters,
        branches: host.state.stepEditor.race_branches.filter(Boolean).map(nodeRef),
        winner: host.state.stepEditor.race_winner,
      };
    }

    if (next.kind === "output") {
      const data = parseStepJson("Output data", host.state.stepEditor.output_data_json);

      if (!data.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        event_type: host.state.stepEditor.output_event_type.trim() || "workflow.output",
        data: data.value,
      };
    }

    if (next.kind === "input") {
      next.parameters = {
        ...parameters,
        prompt: host.state.stepEditor.input_prompt.trim() || "Provide input",
      };
    }

    if (next.kind === "config") {
      const name = parseStepJson("Config name", host.state.stepEditor.config_name_json);

      if (!name.ok) {
        return false;
      }

      const metadata = parseStepJson("Config metadata", host.state.stepEditor.config_metadata_json);

      if (!metadata.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        name: name.value,
        metadata: metadata.value,
      };
    }

    if (next.kind === "subflow") {
      const subflowParameters = parseRequiredObject(host.state.stepEditor.subflow_parameters_json);

      if (!subflowParameters) {
        setStepEditorError("Subflow parameters must be a JSON object");
        return false;
      }

      if (!host.state.stepEditor.subflow_id.trim()) {
        setStepEditorError("Subflow workflow id is required");
        return false;
      }

      next.subflow_id = host.state.stepEditor.subflow_id.trim();
      next.parameters = subflowParameters;
    } else {
      delete next.subflow_id;
    }

    if (next.kind === "assert") {
      const assertions: JsonRecord[] = [];

      for (const [assertIndex, assertion] of host.state.stepEditor.assert_assertions.entries()) {
        const condition = parseStepJson(
          `Assertion ${String(assertIndex + 1)} condition`,
          assertion.condition_json,
        );

        if (!condition.ok) {
          return false;
        }

        const serialized: JsonRecord = { condition: condition.value };

        if (assertion.name.trim()) {
          serialized.name = assertion.name.trim();
        }

        if (assertion.message.trim()) {
          serialized.message = assertion.message.trim();
        }

        assertions.push(serialized);
      }

      next.parameters = { ...parameters, assertions };
    }

    if (next.kind === "transform") {
      const bindings = parseRequiredObject(host.state.stepEditor.transform_bindings_json);

      if (!bindings) {
        setStepEditorError("Transform bindings must be a JSON object");
        return false;
      }

      next.parameters = { ...parameters, bindings };
    }

    if (next.kind === "audit") {
      const action = parseStepJson("Audit action", host.state.stepEditor.audit_action_json);

      if (!action.ok) {
        return false;
      }

      next.parameters = { ...parameters, action: action.value };
      const optionalAudit: JsonRecord = {};

      for (const [field, text] of [
        ["actor", host.state.stepEditor.audit_actor_json],
        ["target", host.state.stepEditor.audit_target_json],
        ["reason", host.state.stepEditor.audit_reason_json],
      ] as const) {
        const parsed = parseOptionalExpr(`Audit ${field}`, text);

        if (!parsed.ok) {
          return false;
        }

        if (parsed.value !== undefined) {
          optionalAudit[field] = parsed.value;
        }
      }

      next.parameters = { ...next.parameters, ...optionalAudit };
    }

    if (next.kind === "checkpoint") {
      if (!host.state.stepEditor.checkpoint_name.trim()) {
        setStepEditorError("Checkpoint needs a name");
        return false;
      }

      next.parameters = { ...parameters, name: host.state.stepEditor.checkpoint_name.trim() };
    }

    if (next.kind === "mutex") {
      if (!host.state.stepEditor.mutex_name.trim()) {
        setStepEditorError("Mutex needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.mutex_name.trim(),
        poll_interval_seconds: Math.max(1, host.state.stepEditor.mutex_poll_interval),
      };
    }

    if (next.kind === "throttle") {
      if (!host.state.stepEditor.throttle_name.trim()) {
        setStepEditorError("Throttle needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.throttle_name.trim(),
        max_per_window: Math.max(1, host.state.stepEditor.throttle_max_per_window),
        window_seconds: Math.max(1, host.state.stepEditor.throttle_window_seconds),
        poll_interval_seconds: Math.max(1, host.state.stepEditor.throttle_poll_interval),
      };
    }

    if (next.kind === "await_run") {
      const runIds = parseStepJson("Await run ids", host.state.stepEditor.await_run_ids_json);

      if (!runIds.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        run_ids: runIds.value,
        mode: host.state.stepEditor.await_mode === "any" ? "any" : "all",
        poll_interval_seconds: Math.max(1, host.state.stepEditor.await_poll_interval),
      };
    }

    if (next.kind === "debounce") {
      if (!host.state.stepEditor.debounce_name.trim()) {
        setStepEditorError("Debounce needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.debounce_name.trim(),
        delay_seconds: Math.max(1, host.state.stepEditor.debounce_delay_seconds),
      };
      const triggerKey = parseOptionalExpr(
        "Debounce trigger key",
        host.state.stepEditor.debounce_trigger_key_json,
      );

      if (!triggerKey.ok) {
        return false;
      }

      if (triggerKey.value === undefined) {
        delete next.parameters.trigger_key;
      } else {
        next.parameters.trigger_key = triggerKey.value;
      }
    }

    if (next.kind === "collect") {
      if (!host.state.stepEditor.collect_name.trim()) {
        setStepEditorError("Collect needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.collect_name.trim(),
        max: Math.max(1, host.state.stepEditor.collect_max),
      };
    }

    if (next.kind === "barrier") {
      if (!host.state.stepEditor.barrier_name.trim()) {
        setStepEditorError("Barrier needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.barrier_name.trim(),
        count: Math.max(1, host.state.stepEditor.barrier_count),
        poll_interval_seconds: Math.max(1, host.state.stepEditor.barrier_poll_interval),
      };
    }

    if (next.kind === "circuit_breaker") {
      if (!host.state.stepEditor.circuit_name.trim()) {
        setStepEditorError("Circuit breaker needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: host.state.stepEditor.circuit_name.trim(),
        threshold: Math.max(1, host.state.stepEditor.circuit_threshold),
        window_seconds: Math.max(1, host.state.stepEditor.circuit_window_seconds),
        cooldown_seconds: Math.max(0, host.state.stepEditor.circuit_cooldown_seconds),
      };
    }

    if (next.kind === "event_source") {
      next.parameters = { ...parameters, event_type: host.state.stepEditor.event_source_type.trim() || "*" };
      const filter = parseOptionalExpr("Event source filter", host.state.stepEditor.event_source_filter_json);

      if (!filter.ok) {
        return false;
      }

      if (filter.value === undefined) {
        delete next.parameters.filter;
      } else {
        next.parameters.filter = filter.value;
      }

      const max = Math.trunc(host.state.stepEditor.event_source_max);

      if (Number.isFinite(max) && max > 0) {
        next.parameters.max = max;
      } else {
        delete next.parameters.max;
      }
    }

    nodes[index] = next;

    if (host.state.selectedStepId !== next.id) {
      renameLayoutNode(host.state.selectedStepId, next.id);
    }

    host.state.selectedStepId = next.id;
    syncWorkflowDraftToJson();
    return true;
  }

  function populateStepEditor(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node) {
      return;
    }

    const parameters = asRecord(node.parameters);
    const transitions = asRecord(node.transitions);
    const wait = asRecord(node.wait);
    const retry = asRecord(node.retry);
    internal.stepEditorHydrating = true;

    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    host.state.selectedStepId = nodeId;
    host.state.stepEditor.id = nodeId;
    host.state.stepEditor.name = displayValue(node.name);
    host.state.stepEditor.kind = displayValue(node.kind) || "action";
    host.state.stepEditor.approval_type = displayValue(parameters.approval_type) || "generic";
    host.state.stepEditor.approval_prompt = displayValue(parameters.prompt) || "Approval required";
    host.state.stepEditor.gate_kind = displayValue(parameters.kind) || "manual";
    host.state.stepEditor.gate_when_json = pretty(parameters.when ?? {});
    host.state.stepEditor.gate_poll_interval = Number(parameters.poll_interval ?? 30);
    host.state.stepEditor.gate_timeout = Number(parameters.timeout ?? 0);
    host.state.stepEditor.gate_label = displayValue(parameters.label);
    host.state.stepEditor.signal_name = displayValue(parameters.name) || "signal";
    host.state.stepEditor.condition_fallback = nodeRefId(transitions.next) ?? "";
    host.state.stepEditor.condition_branches = asArray(transitions.branches).map((branch) => {
      const record = asRecord(branch);
      return { when_json: pretty(record.when ?? {}), target: nodeRefId(record.target) ?? "" };
    });
    host.state.stepEditor.wait_seconds = Number(wait.seconds ?? 60);
    host.state.stepEditor.wait_initial_status = displayValue(wait.initial_status) || "waiting";
    host.state.stepEditor.wait_until_status = displayValue(wait.until_status);
    host.state.stepEditor.wait_json = pretty(node.wait ?? {});
    host.state.stepEditor.loop_items_json = pretty(parameters.items ?? []);
    host.state.stepEditor.loop_target = nodeRefId(parameters.target) ?? "";
    host.state.stepEditor.loop_max_iterations = Number(node.max_iterations ?? 10);
    host.state.stepEditor.switch_value_json = pretty(parameters.value ?? valueRef("params", ["mode"]));
    host.state.stepEditor.switch_cases = asArray(parameters.cases).map((value) =>
      switchCaseEditor(asRecord(value)),
    );
    host.state.stepEditor.switch_default = nodeRefId(parameters.default) ?? "";
    host.state.stepEditor.toggle_value_json = pretty(
      parameters.value ?? valueRef("config", ["flags", "enabled"]),
    );
    host.state.stepEditor.toggle_on = nodeRefId(parameters.on) ?? "";
    host.state.stepEditor.toggle_off = nodeRefId(parameters.off) ?? "";
    host.state.stepEditor.percentage_key_json = pretty(parameters.key ?? valueRef("input", ["user_id"]));
    host.state.stepEditor.percentage_buckets = asArray(parameters.buckets).map((bucket) => {
      const record = asRecord(bucket);
      return { weight: Number(record.weight ?? 0), target: nodeRefId(record.target) ?? "" };
    });
    host.state.stepEditor.percentage_default = nodeRefId(parameters.default) ?? "";
    host.state.stepEditor.parallel_branches = nodeRefArray(parameters.branches);
    host.state.stepEditor.join_wait_for = nodeRefArray(parameters.wait_for);
    host.state.stepEditor.join_mode = branchPolicyName(parameters.mode, "all");
    host.state.stepEditor.try_body = nodeRefId(parameters.body) ?? "";
    host.state.stepEditor.try_catch = nodeRefId(parameters.catch) ?? "";
    host.state.stepEditor.try_finally = nodeRefId(parameters.finally) ?? "";
    host.state.stepEditor.map_items_json = pretty(parameters.items ?? []);
    host.state.stepEditor.map_target = nodeRefId(parameters.target) ?? "";
    host.state.stepEditor.map_concurrency = Number(parameters.concurrency ?? 1);
    host.state.stepEditor.race_branches = nodeRefArray(parameters.branches);
    host.state.stepEditor.race_winner = branchPolicyName(parameters.winner, "first_success");
    host.state.stepEditor.output_event_type = displayValue(parameters.event_type) || "workflow.output";
    host.state.stepEditor.output_data_json = stepEditorJson(parameters.data ?? null);
    host.state.stepEditor.input_prompt = displayValue(parameters.prompt) || "Provide input";
    host.state.stepEditor.config_name_json = stepEditorJson(parameters.name ?? "");
    host.state.stepEditor.config_metadata_json = stepEditorJson(parameters.metadata ?? {});
    host.state.stepEditor.subflow_id = displayValue(node.subflow_id);
    host.state.stepEditor.subflow_parameters_json = pretty(node.parameters ?? {});
    host.state.stepEditor.assert_assertions = asArray(parameters.assertions).map((assertion) => {
      const record = asRecord(assertion);
      return {
        name: displayValue(record.name),
        condition_json: pretty(record.condition ?? true),
        message: displayValue(record.message),
      };
    });
    host.state.stepEditor.transform_bindings_json = pretty(parameters.bindings ?? {});
    host.state.stepEditor.audit_action_json = stepEditorJson(parameters.action ?? "workflow.audit");
    host.state.stepEditor.audit_actor_json = optionalExprJson(parameters.actor);
    host.state.stepEditor.audit_target_json = optionalExprJson(parameters.target);
    host.state.stepEditor.audit_reason_json = optionalExprJson(parameters.reason);
    host.state.stepEditor.checkpoint_name = displayValue(parameters.name);
    host.state.stepEditor.mutex_name = displayValue(parameters.name);
    host.state.stepEditor.mutex_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    host.state.stepEditor.throttle_name = displayValue(parameters.name);
    host.state.stepEditor.throttle_max_per_window = Number(parameters.max_per_window ?? 10);
    host.state.stepEditor.throttle_window_seconds = Number(parameters.window_seconds ?? 60);
    host.state.stepEditor.throttle_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    host.state.stepEditor.await_run_ids_json = pretty(parameters.run_ids ?? valueRef("params", ["run_ids"]));
    host.state.stepEditor.await_mode = parameters.mode === "any" ? "any" : "all";
    host.state.stepEditor.await_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    host.state.stepEditor.debounce_name = displayValue(parameters.name);
    host.state.stepEditor.debounce_delay_seconds = Number(parameters.delay_seconds ?? 30);
    host.state.stepEditor.debounce_trigger_key_json = optionalExprJson(parameters.trigger_key);
    host.state.stepEditor.collect_name = displayValue(parameters.name);
    host.state.stepEditor.collect_max = Number(parameters.max ?? 10);
    host.state.stepEditor.barrier_name = displayValue(parameters.name);
    host.state.stepEditor.barrier_count = Number(parameters.count ?? 2);
    host.state.stepEditor.barrier_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    host.state.stepEditor.circuit_name = displayValue(parameters.name);
    host.state.stepEditor.circuit_threshold = Number(parameters.threshold ?? 5);
    host.state.stepEditor.circuit_window_seconds = Number(parameters.window_seconds ?? 60);
    host.state.stepEditor.circuit_cooldown_seconds = Number(parameters.cooldown_seconds ?? 60);
    host.state.stepEditor.event_source_type = displayValue(parameters.event_type) || "*";
    host.state.stepEditor.event_source_filter_json = optionalExprJson(parameters.filter);
    host.state.stepEditor.event_source_max = Number(parameters.max ?? 0);
    host.state.stepEditor.locked = isLockedWorkflowNode(node);
    host.state.stepEditor.skipped = node.skipped === true;
    host.state.stepEditor.max_attempts = Number(retry.max_attempts ?? 1);
    host.state.stepEditor.timeout_seconds = Number(node.timeout_seconds ?? 0);
    const actionConfig = workflowNodeActionConfig(node);
    host.state.stepEditor.action_name = actionConfig.provider;
    host.state.stepEditor.action_function = actionConfig.action;
    // action nodes carry their inputs in action.configuration (merged with node.parameters); show the effective set.
    const actionInputs =
      node.kind === "action" ? workflowNodeActionInputs(node) : (node.parameters ?? {});
    host.state.stepEditor.parameters_json = pretty(actionInputs);
    host.state.stepEditor.transitions_json = pretty(node.transitions ?? {});
    host.state.workflowInspectorMode = "step";
    void runs.updateSelectedWorkflowNodeDetail();
    setTimeout(() => {
      internal.stepEditorHydrating = false;
    }, 0);
  }

  function workflowEdgeOptions(sourceId: string) {
    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === sourceId);
    return sourceNode ? workflowEdgeSemanticOptions(sourceNode) : [];
  }

  function openEdgeEditorDraft(edgeId: string): WorkflowEdgeEditorDraft | null {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);
    return edge ? workflowEdgeEditorDraft(host.state.workflowDraft, edge) : null;
  }

  function selectGraphEdge(edgeId: string) {
    host.state.selectedStepId = "";
    host.state.selectedGraphEdgeId = edgeId;
  }

  function applyEdgeEditorDraft(draft: WorkflowEdgeEditorDraft): boolean {
    const previousEdge = draft.edgeId
      ? (host.buildDraftGraphEdges().find((edge: GraphEdgeModel) => edge.id === draft.edgeId) ?? null)
      : null;
    const result = applyWorkflowEdgeEditorDraft(host.state.workflowDraft.definition, previousEdge, draft);

    if (!result.ok) {
      host.ctx.setError(result.message);
      return false;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    return true;
  }

  function moveEdgeEditorItem(
    draft: WorkflowEdgeEditorDraft,
    direction: -1 | 1,
  ): WorkflowEdgeEditorDraft | null {
    const result = moveWorkflowEdgeEditorDraft(host.state.workflowDraft.definition, draft, direction);

    if (!result.ok) {
      host.ctx.setError(result.message);
      return null;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    const movedEdge = host.buildDraftGraphEdges().find(
      (edge: GraphEdgeModel) =>
        edge.source === result.draft.source &&
        edge.target === result.draft.target &&
        workflowEdgeOptionId(edge) === result.draft.optionId,
    );
    return movedEdge ? { ...result.draft, edgeId: movedEdge.id } : result.draft;
  }

  function moveSelectedEdge(direction: -1 | 1): boolean {
    const draft = host.state.selectedGraphEdgeId ? openEdgeEditorDraft(host.state.selectedGraphEdgeId) : null;

    if (!draft) {
      return false;
    }

    const moved = moveEdgeEditorItem(draft, direction);

    if (!moved) {
      return false;
    }

    host.state.selectedGraphEdgeId = moved.edgeId;
    return true;
  }

  function reverseSelectedEdgeHandles(): boolean {
    const edge = host.getSelectedGraphEdge();

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    const data = edge.data as WorkflowEditorEdgeData | undefined;
    const semanticKey =
      data?.transitionKey ??
      (typeof data?.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    setWorkflowEdgeHandles(
      host.state.workflowDraft.definition,
      edge.source,
      semanticKey,
      edge.targetHandle,
      edge.sourceHandle,
      data?.edgeStyle,
    );
    syncWorkflowDraftToJson();
    host.state.selectedGraphEdgeId = "";
    return true;
  }

  function setEdgeLabelOffset(edgeId: string, offset: { x: number; y: number } | null): boolean {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelOffset(host.state.workflowDraft.definition, edge, offset);
    syncWorkflowDraftToJson();
    return true;
  }

  function setEdgeLabelAnchor(edgeId: string, position: number | null): boolean {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelAnchor(
      host.state.workflowDraft.definition,
      edge,
      position === null ? null : { position },
    );
    syncWorkflowDraftToJson();
    return true;
  }

  function scheduleStepEditorApply() {
    void applyStepEditor();
  }

  function applyGraphEdgeSemantic(
    connection: GraphEdgeLike,
    optionId: string,
    previousEdgeId = "",
  ): boolean {
    const { source, target, sourceHandle } = connection;

    if (!source || !target) {
      return false;
    }

    dismissStepEditorForCanvasEdit();

    if (isSameConnectionPointLoop(connection)) {
      host.ctx.setError("Cannot connect a node handle back to itself");
      return false;
    }

    const previousEdge = previousEdgeId
      ? (host.buildDraftGraphEdges().find((edge: GraphEdgeModel) => edge.id === previousEdgeId) ?? null)
      : null;
    const previousDraft = previousEdge
      ? workflowEdgeEditorDraft(host.state.workflowDraft, previousEdge)
      : null;
    const draft: WorkflowEdgeEditorDraft = {
      ...(previousDraft ?? defaultEdgeEditorDraft()),
      edgeId: previousEdgeId,
      source,
      target,
      optionId,
      sourceHandle,
      targetHandle: connection.targetHandle,
    };
    return applyEdgeEditorDraft(draft);
  }

  function removeWorkflowEdgeById(edgeId: string) {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return;
    }

    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === edge.source);

    if (!sourceNode || !removeWorkflowEdge(sourceNode, edge)) {
      return;
    }

    const data = edge.data as WorkflowEditorEdgeData | undefined;

    if (data?.transitionKey) {
      removeWorkflowEdgeHandles(host.state.workflowDraft.definition, edge.source, data.transitionKey);
    }

    if (typeof data?.branchIndex === "number") {
      removeWorkflowEdgeHandles(
        host.state.workflowDraft.definition,
        edge.source,
        `branches.${String(data.branchIndex)}`,
      );
    }

    if (data?.parameterKey) {
      removeWorkflowEdgeHandles(
        host.state.workflowDraft.definition,
        edge.source,
        parameterSemanticKey(data.parameterKey, data.parameterIndex),
      );
    }

    syncWorkflowDraftToJson();

    if (host.state.selectedStepId) {
      populateStepEditor(host.state.selectedStepId);
    }
  }

  function autoArrangeWorkflowNodes(
    direction: WorkflowLayoutDirection = host.state.workflowLayoutDirection,
  ) {
    if (!syncWorkflowJson()) {
      return;
    }

    host.state.workflowLayoutDirection = direction;
    const positions = autoArrangeWorkflowLayout(host.state.workflowDraft.definition, direction);

    for (const [nodeId, position] of Object.entries(positions)) {
      setGraphNodePosition(nodeId, position);
    }

    autoArrangeWorkflowEdgeHandles(host.state.workflowDraft.definition, positions);
    host.state.workflowLayoutVersion += 1;
    syncWorkflowDraftToJson();
  }

  function scheduleWorkflowJsonSync() {
    void syncWorkflowJson();
  }

  function scheduleWorkflowWdlSync() {
    if (internal.workflowWdlSyncTimer) {
      clearTimeout(internal.workflowWdlSyncTimer);
    }

    internal.workflowWdlSyncTimer = setTimeout(() => {
      internal.workflowWdlSyncTimer = null;
      void syncWorkflowWdl();
    }, WORKFLOW_WDL_SYNC_DELAY_MS);
  }

  function scheduleWorkflowWdlRefresh() {
    void refreshWorkflowWdl();
  }

  function setWorkflowJsonSilently(next: string) {
    if (internal.workflowJsonWriteReleaseTimer) {
      clearTimeout(internal.workflowJsonWriteReleaseTimer);
    }

    internal.workflowJsonWriteGuard = true;
    host.state.workflowJson = next;
    host.notify();
    internal.workflowJsonWriteReleaseTimer = setTimeout(() => {
      internal.workflowJsonWriteGuard = false;
      internal.workflowJsonWriteReleaseTimer = null;
    }, 0);
  }

  function setWorkflowWdlSilently(next: string) {
    if (internal.workflowWdlWriteReleaseTimer) {
      clearTimeout(internal.workflowWdlWriteReleaseTimer);
    }

    internal.workflowWdlWriteGuard = true;
    host.state.workflowWdl = next;
    host.notify();
    internal.workflowWdlWriteReleaseTimer = setTimeout(() => {
      internal.workflowWdlWriteGuard = false;
      internal.workflowWdlWriteReleaseTimer = null;
    }, 0);
  }

  function syncWorkflowJson(): boolean {
    const parsed = parseRequiredObject(host.state.workflowJson);

    if (!parsed) {
      host.ctx.setError("Workflow definition must be a JSON object");
      return false;
    }

    const errors = validateWorkflowReferenceSyntax(parsed);

    if (errors.length > 0) {
      host.ctx.setError(errors[0]);
      return false;
    }

    host.state.workflowDraft.definition = parsed;
    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
    return true;
  }

  function syncWorkflowDraftToJson() {
    // a graph edit is now the source of truth, so save should serialize the draft, not recompile wdl.
    host.state.workflowEditorMode = "graph";
    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
  }

  async function syncWorkflowWdl(): Promise<boolean> {
    if (internal.workflowWdlSyncTimer) {
      clearTimeout(internal.workflowWdlSyncTimer);
      internal.workflowWdlSyncTimer = null;
    }

    let compiled: WorkflowDefinition;
    const previousUi = isJsonObject(host.state.workflowDraft.definition.ui)
      ? cloneJson(host.state.workflowDraft.definition.ui)
      : null;

    try {
      compiled = await compileWdl(host.state.workflowWdl, host.state.workflowDraft.enabled);
    } catch (err) {
      host.ctx.setError(`WDL compile error: ${errorMessage(err)}`);
      return false;
    }

    host.state.workflowDraft.name = compiled.name;
    host.state.workflowDraft.version = compiled.version;
    host.state.workflowDraft.input_type = compiled.input_type;
    host.state.workflowDraft.definition = compiled.definition;

    if (previousUi) {
      host.state.workflowDraft.definition.ui = previousUi;
    }

    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    return true;
  }

  async function refreshWorkflowWdl(): Promise<void> {
    try {
      setWorkflowWdlSilently(await decompileToWdl(cloneJson(host.state.workflowDraft)));
      host.state.workflowWdlError = "";
    } catch (err) {
      setWorkflowWdlSilently("");
      host.state.workflowWdlError = errorMessage(err);
    }
    host.notify();
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!Array.isArray(host.state.workflowDraft.definition.nodes)) {
      host.state.workflowDraft.definition.nodes = [];
    }

    return host.state.workflowDraft.definition.nodes as JsonRecord[];
  }

  function stripNewNodeConnections(node: JsonRecord) {
    const transitions = asRecord(node.transitions);
    const omitTransitionKeys = new Set<string>([...directTransitionKeys, "branches"]);
    node.transitions = Object.fromEntries(
      Object.entries(transitions).filter(([entryKey]) => !omitTransitionKeys.has(entryKey)),
    );

    const parameters = asRecord(node.parameters);
    const omitParameterKeys = new Set(["target", "default", "body", "catch", "finally"]);
    const cleanedParameters = Object.fromEntries(
      Object.entries(parameters).filter(([entryKey]) => !omitParameterKeys.has(entryKey)),
    );

    if (Array.isArray(parameters.cases)) {
      cleanedParameters.cases = [];
    }

    if (Array.isArray(parameters.branches)) {
      cleanedParameters.branches = [];
    }

    if (Array.isArray(parameters.wait_for)) {
      cleanedParameters.wait_for = [];
    }

    node.parameters = cleanedParameters;
  }

  function graphCentroidPosition(): { x: number; y: number } {
    const positioned = host
      .buildDraftGraphNodes()
      .map((node) => ({
        x: node.position.x,
        y: node.position.y,
      }))
      .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));

    if (positioned.length === 0) {
      return nextNodePosition(1);
    }

    const totals = positioned.reduce(
      (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
      { x: 0, y: 0 },
    );
    return {
      x: Math.round(totals.x / positioned.length),
      y: Math.round(totals.y / positioned.length),
    };
  }

  function setGraphNodePosition(nodeId: string, position: { x: number; y: number }) {
    const definition = host.state.workflowDraft.definition;
    const ui = asRecord(definition.ui);
    definition.ui = ui;
    const layout = asRecord(ui.layout);
    ui.layout = layout;
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = layoutNodes;
    layoutNodes[nodeId] = { x: position.x, y: position.y };
  }

  function renameLayoutNode(previousId: string, nextId: string) {
    if (!previousId || previousId === nextId) {
      return;
    }

    const layout = asRecord(asRecord(host.state.workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);

    if (!layoutNodes[previousId]) {
      return;
    }

    const { [previousId]: movedNode, ...remainingNodes } = layoutNodes;
    layout.nodes = { ...remainingNodes, [nextId]: movedNode };
  }

  function addConditionBranchEditor() {
    host.state.stepEditor.condition_branches.push({
      when_json: pretty({ value: valueRef("params", ["value"]), equals: true }),
      target: "",
    });
    markWorkflowDirty();
  }

  function removeConditionBranchEditor(index: number) {
    host.state.stepEditor.condition_branches.splice(index, 1);
    const node = host.getSelectedNode();

    if (node?.kind === "condition") {
      removeConditionBranch(node, index);
    }

    markWorkflowDirty();
  }

  function addSwitchCaseEditor() {
    host.state.stepEditor.switch_cases.push({ match_kind: "equals", match_json: pretty(true), target: "" });
    markWorkflowDirty();
  }

  function removeSwitchCaseEditor(index: number) {
    host.state.stepEditor.switch_cases.splice(index, 1);
    markWorkflowDirty();
  }

  function addAssertionEditor() {
    host.state.stepEditor.assert_assertions.push({
      name: "",
      condition_json: pretty({ value: valueRef("params", ["value"]), equals: true }),
      message: "",
    });
    markWorkflowDirty();
  }

  function removeAssertionEditor(index: number) {
    host.state.stepEditor.assert_assertions.splice(index, 1);
    markWorkflowDirty();
  }

  function addPercentageBucketEditor() {
    host.state.stepEditor.percentage_buckets.push({ weight: 50, target: "" });
    markWorkflowDirty();
  }

  function removePercentageBucketEditor(index: number) {
    host.state.stepEditor.percentage_buckets.splice(index, 1);
    markWorkflowDirty();
  }

  function addNodeRefEditor(list: string[]) {
    list.push("");
    markWorkflowDirty();
  }

  function removeNodeRefEditor(list: string[], index: number) {
    list.splice(index, 1);
    markWorkflowDirty();
  }

  function markWorkflowDirty() {
    host.state.isDirty = true;
  }

  function openStepEditor(nodeId: string, creating = false) {
    internal.stepEditorBaselineDefinition = creating ? null : cloneJson(host.state.workflowDraft.definition);
    populateStepEditor(nodeId);
    host.state.stepEditorCreating = creating;
    host.state.stepEditorCreatedNodeId = creating ? nodeId : "";
    host.state.stepEditorError = "";
    host.state.workflowInspectorMode = "step";
    // the full modal supersedes the inline mini-editor.
    host.state.inlineEditNodeId = "";
    host.state.stepEditorOpen = true;
  }

  async function submitStepEditor() {
    if (!applyStepEditor()) {
      return;
    }

    host.state.stepEditorOpen = false;
    host.state.stepEditorCreating = false;
    host.state.stepEditorCreatedNodeId = "";
    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    // applying a step persists the workflow so canvas edits do not need a manual save.
    await catalog.saveSelectedWorkflowBundle();
  }

  function dismissStepEditorForCanvasEdit() {
    if (!host.state.stepEditorOpen || host.state.stepEditorCreating) {
      return;
    }

    host.state.stepEditorOpen = false;
    host.state.stepEditorError = "";
  }

  function closeStepEditor() {
    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    if (host.state.stepEditorCreating && host.state.stepEditorCreatedNodeId) {
      const nodeId = host.state.stepEditorCreatedNodeId;
      host.state.workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
        (node: JsonRecord) => node.id !== nodeId,
      );
      syncWorkflowDraftToJson();
    } else if (internal.stepEditorBaselineDefinition) {
      host.state.workflowDraft.definition = cloneJson(internal.stepEditorBaselineDefinition);
      syncWorkflowDraftToJson();
    }

    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    host.state.stepEditorOpen = false;
    host.state.stepEditorCreating = false;
    host.state.stepEditorCreatedNodeId = "";
    host.state.stepEditorError = "";
    internal.stepEditorBaselineDefinition = null;
    internal.stepEditorHydrating = false;
  }

  function duplicateSelectedStep() {
    if (!host.state.selectedStepId || !host.canRemoveSelectedStep()) {
      return;
    }

    const nodes = ensureWorkflowNodes();
    const source = nodes.find((node: JsonRecord) => node.id === host.state.selectedStepId);

    if (!source) {
      return;
    }

    const copy = cloneJson(source);
    const copyId = uniqueWorkflowNodeId(nodes, `${String(source.id)}_copy`);
    copy.id = copyId;
    stripNewNodeConnections(copy);
    const position = graphCentroidPosition();
    nodes.push(copy);
    setGraphNodePosition(copyId, position);
    syncWorkflowDraftToJson();
    populateStepEditor(copyId);
    openStepEditor(copyId, true);
  }

  function setStepEditorError(message: string) {
    host.state.stepEditorError = message;
    host.ctx.setError(message);
    host.notify();
  }

  function parseStepJson(
    label: string,
    text: string,
  ): { ok: true; value: JsonValue } | { ok: false } {
    const value = parseRequiredJson(text);

    if (value !== null || text.trim() === "null") {
      return { ok: true, value: value ?? null };
    }

    setStepEditorError(`${label} must be valid JSON`);
    return { ok: false };
  }

  function stepEditorJson(value: unknown): string {
    return JSON.stringify(value === undefined ? null : asJsonValue(value), null, 2);
  }

  function optionalExprJson(value: unknown): string {
    return value === undefined || value === null ? "" : pretty(asJsonValue(value));
  }

  function parseOptionalExpr(
    label: string,
    text: string,
  ): { ok: true; value: JsonValue | undefined } | { ok: false } {
    if (text.trim() === "") {
      return { ok: true, value: undefined };
    }

    const parsed = parseStepJson(label, text);
    return parsed.ok ? { ok: true, value: parsed.value } : { ok: false };
  }

  function isJsonObject(value: unknown): value is JsonRecord {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function validateStepParameters(parameters: JsonRecord): string {
    if (host.state.stepEditor.kind !== "action") {
      return "";
    }

    const provider = host.getProviders().find((item) => item.name === host.state.stepEditor.action_name);
    const action = provider?.actions.find(
      (item) => item.function_name === host.state.stepEditor.action_function,
    );

    if (!action) {
      return "Select a valid task provider action";
    }

    for (const parameter of action.parameters) {
      if (!parameter.required) {
        continue;
      }

      const value = parameters[parameter.name];

      if (isBlankValue(value)) {
        return `${parameter.label ?? parameter.name} is required`;
      }

      const typeError = validateJsonValueType(
        value,
        parameter.ty,
        parameter.label ?? parameter.name,
      );

      if (typeError) {
        return typeError;
      }
    }

    return "";
  }

  return { addWorkflowStep, addWorkflowNode, addConnectedWorkflowNode, removeWorkflowStep, removeWorkflowNode, applyInlineNodeEdit, clearWorkflowGraphSelection, submitInlineNodeEdit, applyStepEditor, populateStepEditor, workflowEdgeOptions, openEdgeEditorDraft, selectGraphEdge, applyEdgeEditorDraft, moveEdgeEditorItem, moveSelectedEdge, reverseSelectedEdgeHandles, setEdgeLabelOffset, setEdgeLabelAnchor, scheduleStepEditorApply, applyGraphEdgeSemantic, removeWorkflowEdgeById, autoArrangeWorkflowNodes, scheduleWorkflowJsonSync, scheduleWorkflowWdlSync, scheduleWorkflowWdlRefresh, setWorkflowJsonSilently, setWorkflowWdlSilently, syncWorkflowJson, syncWorkflowDraftToJson, syncWorkflowWdl, refreshWorkflowWdl, ensureWorkflowNodes, stripNewNodeConnections, graphCentroidPosition, setGraphNodePosition, renameLayoutNode, addConditionBranchEditor, removeConditionBranchEditor, addSwitchCaseEditor, removeSwitchCaseEditor, addAssertionEditor, removeAssertionEditor, addPercentageBucketEditor, removePercentageBucketEditor, addNodeRefEditor, removeNodeRefEditor, markWorkflowDirty, openStepEditor, submitStepEditor, dismissStepEditorForCanvasEdit, closeStepEditor, duplicateSelectedStep, setStepEditorError, parseStepJson, stepEditorJson, optionalExprJson, parseOptionalExpr, isJsonObject, validateStepParameters };
}
