import { computed, ref, watch } from "vue";
import { workflowRunExtrasService } from "../../core/services";
import { isApprovalWaitingStatus, type ApprovalAction } from "../../core/utils/approvals";
import { isInputWaitingStatus } from "../../core/utils/inputs";
import { displayValue } from "../../core/utils/values";
import { useAppStore } from "../adapters/pinia/app";
import { useResourcesStore } from "../adapters/pinia/resources";
import { useWorkflowsStore } from "../adapters/pinia/workflows";
import type { WorkflowNodeData } from "../components/workflow/workflow-node-types";

interface WorkflowNodeRuntimeProps {
  id: string;
  data: WorkflowNodeData;
}

/** runtime-only controls for parked workflow nodes. */
export function useWorkflowNodeRuntime(props: WorkflowNodeRuntimeProps) {
  const app = useAppStore();
  const resources = useResourcesStore();
  const workflows = useWorkflowsStore();
  const submitting = ref(false);
  const inputDraft = ref("{}");
  const inputError = ref("");
  const signalPayloadDraft = ref("{}");
  const signalError = ref("");
  const gateReasonDraft = ref("");

  const isApprovalPending = computed(() => isApprovalWaitingStatus(props.data.status));
  const isInputPending = computed(() => isInputWaitingStatus(props.data.status));
  const isSignalPending = computed(
    () => props.data.kind === "signal" && props.data.status === "waiting",
  );
  const gate = computed(() => props.data.gate ?? null);
  const gateKind = computed(() => gate.value?.kind ?? "");
  const gateStatus = computed(() => gate.value?.status ?? "");
  const gateKindLabel = computed(() => (gateKind.value ? `${gateKind.value} gate` : "gate"));
  const gateStatusLabel = computed(() => gateStatus.value || "waiting");
  const gateReasonText = computed(() => (gate.value?.reason ?? "").trim());
  const gateStateText = computed(() => props.data.kind === "gate" && Boolean(gate.value));
  const isConditionGate = computed(() => gateKind.value === "condition");
  const canResolveGate = computed(() => {
    if (props.data.kind !== "gate" || !props.data.allowGateResolution || !gate.value?.id) {
      return false;
    }

    return (
      ["manual", "external"].includes(gateKind.value) &&
      ["pending", "closed"].includes(gateStatus.value)
    );
  });
  const isWaiting = computed(() => isApprovalPending.value || isInputPending.value);

  watch(
    () => gate.value?.id,
    () => {
      gateReasonDraft.value = "";
    },
  );

  watch(
    () => [
      props.id,
      workflows.workflowRunDetail?.nodes
        .filter((node) => node.node_id === props.id)
        .at(-1)?.status,
    ],
    () => {
      const nodeRun = workflows.workflowRunDetail?.nodes
        .filter((node) => node.node_id === props.id && isInputWaitingStatus(node.status))
        .at(-1);

      if (!nodeRun) {
        return;
      }

      inputDraft.value = formatInputDraft(nodeRun.output_json ?? nodeRun.state?.input ?? {});
      inputError.value = "";
    },
    { immediate: true },
  );

  function onInputDraftChange(value: string) {
    inputDraft.value = value;
    inputError.value = "";
  }

  function onSignalPayloadChange(value: string) {
    signalPayloadDraft.value = value;
    signalError.value = "";
  }

  async function resolveApproval(action: ApprovalAction) {
    const detail = workflows.workflowRunDetail;

    if (!detail) {
      app.setError("No workflow run selected");
      return;
    }

    const nodeRun = detail.nodes
      .filter((node) => node.node_id === props.id && isApprovalWaitingStatus(node.status))
      .at(-1);

    if (!nodeRun) {
      app.setError(`No pending approval found for workflow node ${props.id}`);
      return;
    }

    submitting.value = true;

    try {
      await resources.resolveWorkflowApproval(detail.run.id, props.id, nodeRun, action);
      await workflows.fetchWorkflowRunDetail(detail.run.id);
    } finally {
      submitting.value = false;
    }
  }

  async function onSendSignal() {
    const detail = workflows.workflowRunDetail;

    if (!detail) {
      app.setError("No workflow run selected");
      return;
    }

    const nodeRun = detail.nodes
      .filter((node) => node.node_id === props.id && node.status === "waiting")
      .at(-1);

    if (!nodeRun) {
      app.setError(`No waiting signal found for node ${props.id}`);
      return;
    }

    const name = displayValue(nodeRun.state?.name ?? "");

    if (!name) {
      app.setError(`Signal node ${props.id} has no signal name`);
      return;
    }

    let payload: unknown;

    try {
      payload = JSON.parse(signalPayloadDraft.value || "{}");
      signalError.value = "";
    } catch (err) {
      signalError.value = String(err);
      return;
    }

    submitting.value = true;

    try {
      await workflowRunExtrasService.deliverSignal(detail.run.id, name, payload);
      await workflows.fetchWorkflowRunDetail(detail.run.id);
    } finally {
      submitting.value = false;
    }
  }

  async function onSubmitInput() {
    const detail = workflows.workflowRunDetail;

    if (!detail) {
      app.setError("No workflow run selected");
      return;
    }

    const nodeRun = detail.nodes
      .filter((node) => node.node_id === props.id && isInputWaitingStatus(node.status))
      .at(-1);

    if (!nodeRun) {
      app.setError(`No pending input found for workflow node ${props.id}`);
      return;
    }

    let parsed: unknown;

    try {
      parsed = JSON.parse(inputDraft.value || "null");
      inputError.value = "";
    } catch (err) {
      inputError.value = String(err);
      return;
    }

    submitting.value = true;

    try {
      await workflowRunExtrasService.resolveInput(nodeRun.id, parsed, undefined, "Input submitted");
      await workflows.fetchWorkflowRunDetail(detail.run.id);
    } finally {
      submitting.value = false;
    }
  }

  async function onResolveGate(action: "open" | "close") {
    const gateId = gate.value?.id ?? "";

    if (!gateId) {
      app.setError(`No gate found for workflow node ${props.id}`);
      return;
    }

    submitting.value = true;

    try {
      await workflows.resolveWorkflowRunGate(gateId, action, gateReasonDraft.value);
      gateReasonDraft.value = "";
    } finally {
      submitting.value = false;
    }
  }

  return {
    submitting,
    inputDraft,
    inputError,
    signalPayloadDraft,
    signalError,
    gateReasonDraft,
    isApprovalPending,
    isInputPending,
    isSignalPending,
    gateKindLabel,
    gateStatusLabel,
    gateReasonText,
    gateStateText,
    isConditionGate,
    canResolveGate,
    isWaiting,
    onInputDraftChange,
    onSignalPayloadChange,
    onApprove: () => resolveApproval("approve"),
    onReject: () => resolveApproval("reject"),
    onSendSignal,
    onSubmitInput,
    onResolveGate,
  };
}

function formatInputDraft(value: unknown): string {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch {
    return "{}";
  }
}
