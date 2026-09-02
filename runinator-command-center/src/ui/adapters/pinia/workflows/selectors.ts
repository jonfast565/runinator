import type { Edge, Node } from "@vue-flow/core";
import { computed, ref, type Ref } from "vue";
import type {
  ControlFrame,
  DebugFrame,
  GateRecord,
  JsonRecord,
  ProviderMetadata,
  RunSummary,
  RuninatorType,
  WorkflowDefinition,
  WorkflowNodeRun,
  WorkflowValidationIssue,
} from "../../../../core/domain/models";
import { workflowInputType } from "../../../../core/domain/models";
import type { WorkflowServices } from "../../../../core/services";
import { isCompletedNodeStatus } from "../../../../core/utils/status";
import {
  formatMaybeDate,
  isKindLockedWorkflowNode,
} from "../../../../core/workflow/editor-defaults";
import {
  traversedEdgeKeys,
  workflowNodeKindsList,
  workflowRunSearchText,
} from "../../../../core/workflow";
import { buildGraphEdges, buildGraphNodes } from "../../vue-flow/builder";

type WorkflowState = ReturnType<WorkflowServices["getState"]>;

interface WorkflowSelectorDependencies {
  services: WorkflowServices;
  state: Ref<WorkflowState>;
  catalogState: Readonly<Ref<{ nodeKinds: unknown }>>;
  workflowDraft: WorkflowState["workflowDraft"];
  searchQuery: () => string;
  providerCatalog: () => ProviderMetadata[];
}

/** read-only projections consumed by workflow views and graph components. */
export function createWorkflowSelectors(deps: WorkflowSelectorDependencies) {
  const { services, state, catalogState, workflowDraft } = deps;

  function mirroredComputed<T>(selector: () => T) {
    return computed(() => {
      void state.value;
      void catalogState.value.nodeKinds;
      return selector();
    });
  }

  const selectedWorkflow = mirroredComputed((): WorkflowDefinition | null =>
    services.getSelectedWorkflow(),
  );
  const canRunWorkflow = mirroredComputed(() =>
    Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id),
  );
  const selectedWorkflowInputType = mirroredComputed((): RuninatorType | null =>
    selectedWorkflow.value ? workflowInputType(selectedWorkflow.value) : null,
  );
  const selectedWorkflowHasInputs = mirroredComputed(() => services.selectedWorkflowHasInputs());
  const canManageWorkflowTriggers = mirroredComputed(() => Boolean(workflowDraft.id));
  const canStepWorkflowRun = mirroredComputed(() => services.canStepWorkflowRun());
  const debugState = mirroredComputed<DebugFrame | null>(() => services.getDebugState());
  const isDebugRun = mirroredComputed(() => services.isDebugRun());
  const cursorMarkers = mirroredComputed(() => services.getCursorMarkers());
  const cursors = mirroredComputed(() => services.getCursors());
  const selectedCursorId = mirroredComputed(() => services.getSelectedCursorId());
  const canContinueWorkflowRun = mirroredComputed(() => services.canContinueWorkflowRun());
  const isSelectedCursorPaused = mirroredComputed(() => services.isSelectedCursorPaused());
  const controlState = mirroredComputed<ControlFrame | null>(() => services.getControlState());
  const canPauseWorkflowRun = mirroredComputed(() => services.canPauseWorkflowRun());
  const canResumeWorkflowRun = mirroredComputed(() => services.canResumeWorkflowRun());
  const canCancelWorkflowRun = mirroredComputed(() => services.canCancelWorkflowRun());
  const currentBreakpoints = mirroredComputed<string[]>(() => services.getCurrentBreakpoints());
  const selectedStepKindLocked = mirroredComputed(() => {
    const node = services.getSelectedNode();
    // not the same question as "can it be removed": an interrupt entry is deletable but its kind is
    // fixed, because the node *is* the handler declaration.
    return node ? isKindLockedWorkflowNode(node) : false;
  });
  const canRemoveSelectedStep = mirroredComputed(() => services.canRemoveSelectedStep());
  const filteredWorkflows = mirroredComputed((): WorkflowDefinition[] =>
    services.getFilteredWorkflows(),
  );
  const recentWorkflowRuns = computed((): RunSummary[] => {
    const query = deps.searchQuery();

    if (!query) {
      return state.value.workflowRuns.slice(0, 50);
    }

    return state.value.workflowRuns
      .filter((run) =>
        workflowRunSearchText(run, services.catalog.workflowNameForRun(run)).includes(query),
      )
      .slice(0, 50);
  });
  const workflowRunDetailText = computed(() => {
    const detail = state.value.workflowRunDetail;

    if (!detail) {
      return "";
    }

    const lines = [
      `Run ${detail.run.id}: ${detail.run.status}`,
      `Started: ${formatMaybeDate(detail.run.started_at)}`,
      `Finished: ${formatMaybeDate(detail.run.finished_at)}`,
    ];

    if (detail.run.message) {
      lines.push(`Message: ${detail.run.message}`);
    }

    for (const step of detail.nodes) {
      lines.push(
        `${step.node_id}: ${step.status}, attempt ${String(step.attempt)}, node run ${step.id}${step.message ? `, ${step.message}` : ""}`,
      );
    }

    return `${lines.join("\n")}${state.value.workflowNodeDetailExtra}`;
  });
  const stepNeeds = computed(() => {
    const transitions =
      (state.value.stepEditor.nodeDraft.transitions as JsonRecord | undefined) ?? {};
    return ["next", "on_success", "on_failure", "on_timeout", "on_reject"]
      .filter((key) => transitions[key])
      .map((key) => `${key}:${String(transitions[key])}`)
      .join(",");
  });
  const subflowNames = mirroredComputed(() => services.getSubflowNames());
  const graphNodes = mirroredComputed((): Node[] =>
    buildGraphNodes(workflowDraft, null, subflowNames.value, deps.providerCatalog()),
  );
  const graphEdges = mirroredComputed((): Edge[] => buildGraphEdges(workflowDraft));
  const graphValidationIssues = mirroredComputed((): WorkflowValidationIssue[] =>
    services.getGraphValidationIssues(),
  );
  const workflowRunWorkflow = mirroredComputed((): WorkflowDefinition | null =>
    services.getWorkflowRunWorkflow(),
  );
  const workflowRunGatesByNodeId = computed((): Map<string, GateRecord> => {
    const gates = new Map<string, GateRecord>();

    for (const gate of state.value.workflowRunGates) {
      if (typeof gate.node_id === "string" && gate.node_id.length > 0) {
        gates.set(gate.node_id, gate);
      }
    }

    return gates;
  });
  const runGraphNodes = computed((): Node[] => {
    if (!workflowRunWorkflow.value) {
      return [];
    }

    return buildGraphNodes(
      workflowRunWorkflow.value,
      state.value.workflowRunDetail,
      subflowNames.value,
      deps.providerCatalog(),
      services.getSelectedCursorId(),
    ).map((node) => ({
      ...node,
      data: {
        ...(node.data as JsonRecord),
        readOnly: true,
        allowGateResolution: true,
        gate: workflowRunGatesByNodeId.value.get(node.id) ?? null,
      },
    }));
  });
  const runGraphEdges = computed((): Edge[] => {
    if (!workflowRunWorkflow.value) {
      return [];
    }

    const completed = new Set<string>();
    const active = new Set<string>();

    for (const node of runGraphNodes.value) {
      const data = node.data as JsonRecord | undefined;

      if (isCompletedNodeStatus(data?.status) || data?.skipped === true) {
        completed.add(node.id);
      } else if (data?.running === true) {
        active.add(node.id);
      }
    }

    const walked = traversedEdgeKeys(state.value.workflowRunDetail?.nodes ?? []);
    return buildGraphEdges(workflowRunWorkflow.value, completed, walked, active);
  });
  const selectedNode = mirroredComputed((): JsonRecord | null => services.getSelectedNode());
  const selectedGraphEdge = computed(
    () => graphEdges.value.find((edge) => edge.id === state.value.selectedGraphEdgeId) ?? null,
  );
  const selectedNodeIssues = computed<WorkflowValidationIssue[]>(() =>
    graphValidationIssues.value.filter((issue) => issue.nodeId === state.value.selectedStepId),
  );
  const selectedEdgeIssues = computed<WorkflowValidationIssue[]>(() => {
    const edge = selectedGraphEdge.value;

    if (!edge) {
      return [];
    }

    const data = edge.data as {
      transitionKey?: string;
      branchIndex?: number;
      parameterKey?: string;
      parameterIndex?: number;
    };
    const semanticKey =
      data.transitionKey ??
      (typeof data.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : `${data.parameterKey ?? ""}${data.parameterIndex === undefined ? "" : String(data.parameterIndex)}`);
    return graphValidationIssues.value.filter(
      (issue) => issue.edgeKey === `${edge.source}:${semanticKey}`,
    );
  });
  const selectedNodePendingApproval = computed((): WorkflowNodeRun | null => {
    const detail = state.value.workflowRunDetail;

    if (!detail || !state.value.selectedStepId) {
      return null;
    }

    return (
      detail.nodes
        .filter(
          (node) =>
            node.node_id === state.value.selectedStepId &&
            ["waiting", "approval_required", "pending"].includes(node.status),
        )
        .at(-1) ?? null
    );
  });
  const watchExpressionsForActiveWorkflow = computed<string[]>(() => {
    const workflowId = workflowRunWorkflow.value?.id;
    return workflowId ? (state.value.watchExpressionsByWorkflowId[workflowId] ?? []) : [];
  });
  const followCursor = ref(true);

  return {
    selectedWorkflow,
    canRunWorkflow,
    selectedWorkflowInputType,
    selectedWorkflowHasInputs,
    canManageWorkflowTriggers,
    canStepWorkflowRun,
    debugState,
    isDebugRun,
    cursorMarkers,
    cursors,
    selectedCursorId,
    followCursor,
    setFollowCursor: (value: boolean) => {
      followCursor.value = value;
    },
    canContinueWorkflowRun,
    isSelectedCursorPaused,
    controlState,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    currentBreakpoints,
    selectedStepKindLocked,
    canRemoveSelectedStep,
    filteredWorkflows,
    recentWorkflowRuns,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    graphValidationIssues,
    workflowRunWorkflow,
    runGraphNodes,
    runGraphEdges,
    selectedNode,
    selectedGraphEdge,
    selectedNodeIssues,
    selectedEdgeIssues,
    selectedNodePendingApproval,
    watchExpressionsForActiveWorkflow,
    runDetailById: computed(() => services.internal.runDetailById),
    workflowNodeKinds: computed(() => {
      void catalogState.value.nodeKinds;
      return workflowNodeKindsList();
    }),
  };
}
