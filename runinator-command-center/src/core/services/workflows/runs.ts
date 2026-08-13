import {
  cancelWorkflowRun,
  closeGate,
  continueWorkflowRun,
  createWorkflowRun,
  fetchGates,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  openGate,
  patchWorkflowRunDebug,
  pauseWorkflowRun,
  renameWorkflowRun as renameWorkflowRunApi,
  replayWorkflowRun as replayWorkflowRunApi,
  requestRunInterrupt,
  resumeWorkflowRun,
  rerunWorkflowNode,
  runToCursorWorkflowRun,
  forkWorkflowRunCursor,
  sendDebugCommand,
  skipWorkflowNode,
  stepWorkflowRun,
  type WorkflowDebugPatch,
} from "../../api/commandCenterApi";
import type {
  GateRecord,
  JsonRecord,
  RunArtifact,
  RunChunk,
  RunSummary,
  WorkflowRunDetail,
} from "../../domain/models";
import { asJsonRecord, isJsonRecord, jsonRecordArray } from "../../domain/json";

import { coerceRunCursors, isCursorPaused } from "../../domain/models/workflow-state";
import { coerceDebugFrame } from "../../domain/models/workflow-state";
import { describeBulkResult, runBulk } from "../../utils/bulk";

import { mergeById } from "../../utils/merge";
import { isActiveRunStatus } from "../../utils/status";

import { nodeRef, nodeRefId } from "../../workflow/index";

import { buildInputSkeleton } from "../../workflow/editor-defaults";
import type { WorkflowServiceHost } from "./host";
import { createWorkflowRunWatchService } from "./run-watches";

const MAX_OPEN_RUN_TABS = 8;
const RECENT_RUNS_REFRESH_DEBOUNCE_MS = 300;
// worker status/chunk events often arrive in bursts; coalesce detail refetches so the UI tracks the
// latest node status without stampeding /workflow_runs/:id on every broker result.
const WORKFLOW_RUN_DETAIL_REFRESH_DEBOUNCE_MS = 75;

export function createWorkflowRunService(host: WorkflowServiceHost) {
  const { internal } = host;
  const watchService = createWorkflowRunWatchService(host);
  const asRecord = asJsonRecord;
  const isRecord = isJsonRecord;
  const asArray = jsonRecordArray;

  function isBreakpointed(nodeId: string): boolean {
    return host.getCurrentBreakpoints().includes(nodeId);
  }

  function getTransition(key: string): string {
    const transitions = asRecord(host.state.stepEditor.nodeDraft.transitions);
    return nodeRefId(transitions[key]) ?? "";
  }

  function setTransition(key: string, value: string) {
    const draft = host.state.stepEditor.nodeDraft;
    const transitions = { ...asRecord(draft.transitions) };

    if (value) {
      transitions[key] = nodeRef(value);
    } else {
      Reflect.deleteProperty(transitions, key);
    }

    host.state.stepEditor.nodeDraft = { ...draft, transitions };
    host.state.isDirty = true;
    host.notify();
  }

  async function runSelectedWorkflow(debug = false) {
    const workflow = host.getSelectedWorkflow();

    if (!workflow?.id || !workflow.enabled) {
      host.ctx.setError(workflow ? "Workflow is disabled" : "No workflow selected");
      return;
    }

    if (host.selectedWorkflowHasInputs()) {
      host.state.runInputDraft = buildInputSkeleton(host.getSelectedWorkflowInputType());
      host.state.runInputDebug = debug;
      host.state.runInputOpen = true;
      return;
    }

    await launchWorkflowRun(debug, {});
    host.notify();
  }

  async function runSelectedWorkflowDebug() {
    return runSelectedWorkflow(true);
  }

  function closeRunInput() {
    host.state.runInputOpen = false;
    host.notify();
  }

  async function confirmRunInput() {
    const debug = host.state.runInputDebug;
    const parameters = host.state.runInputDraft;
    host.state.runInputOpen = false;
    await launchWorkflowRun(debug, parameters);
    host.notify();
  }

  async function launchWorkflowRun(debug: boolean, parameters: JsonRecord) {
    const workflow = host.getSelectedWorkflow();

    const workflowId = workflow?.id;

    if (!workflowId || !workflow.enabled) {
      host.ctx.setError(workflow ? "Workflow is disabled" : "No workflow selected");
      return;
    }

    const response = await host.ctx.runOperation(
      debug
        ? `Running workflow ${workflow.name} in debug mode`
        : `Running workflow ${workflow.name}`,
      () => createWorkflowRun(workflowId, { debug, parameters }),
    );
    host.state.selectedWorkflowRunId = response.id;
    host.ctx.setStatus(`${debug ? "Debug workflow run" : "Workflow run"} queued: ${response.id}`);
    await fetchWorkflowRunDetail(response.id);
    await fetchRecentWorkflowRuns();
    host.ctx.activeTab = "Runs";
    host.notify();
  }

  async function stepSelectedWorkflowRun() {
    if (!host.state.workflowRunDetail || !host.canStepWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const cursor = selectedCursorId();
    const response = await host.ctx.runOperation(`Stepping workflow run ${runId}`, () =>
      stepWorkflowRun(runId, cursor),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to step workflow run");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow run ${runId} stepped`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  /**
   * the branch the debugger controls act on: the operator's selection while it is still live, else
   * the first parked one, else the primary. resolved from state rather than by calling back into
   * the service, which would make the service's inferred type circular.
   */
  function selectedCursorId(): string | null {
    const detail = host.state.workflowRunDetail;
    const cursors = coerceRunCursors(detail?.execution_state?.cursors);

    if (cursors.length === 0) {
      return null;
    }

    const chosen = host.state.selectedCursorId;

    if (chosen && cursors.some((cursor) => cursor.id === chosen)) {
      return chosen;
    }

    const frame = coerceDebugFrame(detail?.execution_state?.debug) ?? null;
    const parked = cursors.find((cursor) => isCursorPaused(cursors, cursor.id, frame));
    return (parked ?? cursors.at(0))?.id ?? null;
  }

  /** point the debugger controls at one thread of control. */
  function selectCursor(cursorId: string) {
    host.state.selectedCursorId = cursorId;
    host.notify();
  }

  /** fork a speculative "what if" branch from the selected cursor. */
  async function forkCursor(
    options: { atNode?: string; label?: string; contextPatch?: unknown } = {},
  ) {
    if (!host.state.workflowRunDetail) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Forking a branch of run ${runId}`, () =>
      forkWorkflowRunCursor(runId, {
        fromCursor: selectedCursorId(),
        atNode: options.atNode ?? null,
        label: options.label ?? null,
        contextPatch: options.contextPatch ?? null,
      }),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to fork a speculative branch");
      return;
    }

    // the fork's id comes back as the response message; follow it, since forking is an act of
    // deciding to look at the new branch.
    if (response.message) {
      selectCursor(response.message);
    }

    await fetchWorkflowRunDetail(runId, true);
  }

  /** abandon a speculative branch. */
  async function retireCursor(cursorId: string) {
    if (!host.state.workflowRunDetail) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Retiring branch ${cursorId}`, () =>
      sendDebugCommand(runId, { verb: "retire_cursor", cursor: cursorId }),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to retire the branch");
      return;
    }

    if (host.state.selectedCursorId === cursorId) {
      selectCursor("");
    }

    await fetchWorkflowRunDetail(runId, true);
  }

  /** let a speculative branch dispatch one node for real instead of shadowing it. */
  async function armNodeForReal(cursorId: string, nodeId: string, armed: boolean) {
    if (!host.state.workflowRunDetail) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(
      `${armed ? "Arming" : "Disarming"} ${nodeId}`,
      () =>
        sendDebugCommand(runId, {
          verb: "arm_for_real",
          cursor: cursorId,
          node_id: nodeId,
          armed,
        }),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to change arming");
      return;
    }

    await fetchWorkflowRunDetail(runId, true);
  }

  async function continueSelectedWorkflowRun() {
    if (!host.state.workflowRunDetail || !host.canContinueWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Continuing workflow run ${runId}`, () =>
      continueWorkflowRun(runId, selectedCursorId()),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to continue workflow run");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow run ${runId} continued`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function cancelSelectedWorkflowRun() {
    if (!host.state.workflowRunDetail || !host.canCancelWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Canceling workflow run ${runId}`, () =>
      cancelWorkflowRun(runId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to cancel workflow run");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow run ${runId} canceled`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  /**
   * ask the selected run to raise an interrupt.
   *
   * the web service only records the request; the reducer decides on the next drive whether it can
   * be serviced, and every refusal is silent. so the status message says "recorded", not "raised" --
   * reporting it as raised would be the ui inventing a guarantee the backend does not make.
   */
  async function requestSelectedRunInterrupt(
    source: string,
    payload: unknown = null,
    cursorId: string | null = null,
  ) {
    if (!host.state.workflowRunDetail || !host.canRequestRunInterrupt()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(
      `Requesting ${source} interrupt for workflow run ${runId}`,
      () => requestRunInterrupt(runId, source, payload, cursorId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to request interrupt");
      return;
    }

    host.ctx.setStatus(
      `Interrupt '${source}' recorded on run ${runId}; it is raised on the next drive if a handler can service it`,
    );
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function pauseSelectedWorkflowRun() {
    if (!host.state.workflowRunDetail || !host.canPauseWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Pausing workflow run ${runId}`, () =>
      pauseWorkflowRun(runId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to pause workflow run");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow run ${runId} pause requested`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function resumeSelectedWorkflowRun() {
    if (!host.state.workflowRunDetail || !host.canResumeWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Resuming workflow run ${runId}`, () =>
      resumeWorkflowRun(runId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to resume workflow run");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow run ${runId} resumed`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function patchSelectedWorkflowRunDebug(patch: WorkflowDebugPatch) {
    if (!host.state.workflowRunDetail || !host.isDebugRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Updating debug settings for run ${runId}`, () =>
      patchWorkflowRunDebug(runId, patch),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to update debug settings");
      return;
    }

    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function toggleBreakpoint(nodeId: string) {
    if (!host.state.workflowRunDetail || !host.isDebugRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const current = host.getCurrentBreakpoints();
    const next = current.includes(nodeId)
      ? current.filter((id) => id !== nodeId)
      : [...current, nodeId];
    const mutationId = ++internal.nextBreakpointMutationId;
    internal.pendingBreakpointPatch = { runId, breakpoints: next, mutationId };
    applyBreakpointPatch(host.state.workflowRunDetail, next);

    try {
      const response = await host.ctx.runOperation(`Updating debug settings for run ${runId}`, () =>
        patchWorkflowRunDebug(runId, { breakpoints: next }),
      );

      if (!response.success) {
        host.ctx.setError(response.message || "Failed to update debug settings");

        if (clearPendingBreakpointPatch(runId, mutationId)) {
          applyBreakpointPatch(host.state.workflowRunDetail, current);
        }

        return;
      }

      await fetchWorkflowRunDetail(runId, true);
    } catch {
      if (clearPendingBreakpointPatch(runId, mutationId)) {
        applyBreakpointPatch(host.state.workflowRunDetail, current);
      }
    }

    host.notify();
  }

  async function runToCursor(nodeId: string) {
    if (!host.state.workflowRunDetail || !host.isDebugRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Running to cursor ${nodeId}`, () =>
      runToCursorWorkflowRun(runId, nodeId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to run to cursor");
      return;
    }

    host.ctx.setStatus(response.message || `Running to ${nodeId}`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function skipCurrentNode(outputJson: unknown, message?: string) {
    if (!host.state.workflowRunDetail || !host.canStepWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Skipping current node`, () =>
      skipWorkflowNode(runId, outputJson, message),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to skip node");
      return;
    }

    host.ctx.setStatus(response.message || `Node skipped`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function rerunCurrentNode(parameters: unknown) {
    if (!host.state.workflowRunDetail || !host.canStepWorkflowRun()) {
      return;
    }

    const runId = host.state.workflowRunDetail.run.id;
    const response = await host.ctx.runOperation(`Re-running current node`, () =>
      rerunWorkflowNode(runId, parameters),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to re-run node");
      return;
    }

    host.ctx.setStatus(response.message || `Node re-running`);
    await fetchWorkflowRunDetail(runId, true);
    host.notify();
  }

  async function replaySelectedWorkflowRun(runId?: string, fromStepId?: string) {
    const targetId = runId ?? host.state.workflowRunDetail?.run.id;

    if (!targetId) {
      return;
    }

    const label = fromStepId
      ? `Replaying workflow run ${targetId} from step ${fromStepId}`
      : `Replaying workflow run ${targetId}`;
    const created = await host.ctx
      .runOperation(label, () => replayWorkflowRunApi(targetId, { fromStepId }))
      .catch((error: unknown) => {
        host.ctx.setError(String(error));
        return null;
      });

    if (!created?.id) {
      host.ctx.setError("Failed to start replay");
      return;
    }

    host.ctx.setStatus(`Replay started as run ${created.id}`);
    openRunInTab(created.id);
    activateRunTab(created.id);
    await fetchWorkflowRunDetail(created.id);
    await fetchRecentWorkflowRuns();
    host.ctx.activeTab = "Runs";
    return created.id;
    host.notify();
  }

  async function renameSelectedWorkflowRun(runId: string, name: string | null) {
    if (!runId) {
      return;
    }

    const response = await host.ctx
      .runOperation(`Renaming run ${runId}`, () => renameWorkflowRunApi(runId, name))
      .catch((error: unknown) => {
        host.ctx.setError(String(error));
        return null;
      });

    if (!response) {
      return;
    }

    host.ctx.setStatus(response.message || `Run renamed`);
    await fetchRecentWorkflowRuns();

    if (host.state.workflowRunDetail?.run.id === runId) {
      await fetchWorkflowRunDetail(runId, true);
    }

    host.notify();
  }

  async function fetchWorkflowRunsForSelected(workflowId: string) {
    console.info("[command-center] refreshing workflow runs", { workflowId });
    // resolve before touching host.state: a concurrent notify() elsewhere can swap the state
    // object out from under a `host.state.x = await ...` assignment (the getter reads the object
    // before the await resolves), silently dropping the write onto a detached copy.
    const runs = (await host.ctx
      .runOperation("Loading workflow runs", () => fetchWorkflowRuns(workflowId))
      .catch(() => [])) as RunSummary[];
    host.state.workflowRuns = mergeById(host.state.workflowRuns, runs);

    if (!host.state.workflowRuns.some((run) => run.id === host.state.selectedWorkflowRunId)) {
      host.state.selectedWorkflowRunId = host.state.workflowRuns[0]?.id ?? null;
    }

    host.notify();
  }

  async function fetchRecentWorkflowRuns(options?: { background?: boolean }) {
    console.info("[command-center] refreshing recent workflow runs");
    // background refreshes (poll/event-driven) run silently so the table updates in place instead of
    // dimming; user-initiated refreshes keep the loading indicator.
    const runs = (await host.ctx
      .runOperation("Loading workflow runs", () => fetchWorkflowRuns(), {
        silent: options?.background,
        retryable: true,
      })
      .catch(() => [])) as RunSummary[];
    host.state.workflowRuns = mergeById(host.state.workflowRuns, runs);
    const previousRunId = host.state.selectedWorkflowRunId;

    if (host.state.selectedWorkflowRunId === null && host.state.workflowRuns.length > 0) {
      const first = host.state.workflowRuns[0]?.id ?? null;

      if (first) {
        openRunInTab(first);
        activateRunTab(first);
      }
    }

    const currentRunId = host.state.selectedWorkflowRunId;

    if (
      currentRunId !== null &&
      (!host.state.workflowRunDetail || previousRunId !== currentRunId)
    ) {
      await fetchWorkflowRunDetail(currentRunId, true);
    }

    host.notify();
  }

  // coalesce event-driven recent-runs refetches: a burst of workflow_run_changed events (many node
  // transitions on one run) collapses into a single trailing fetch, and a fetch that arrives mid-flight
  // re-arms once so the final state is never missed. manual refresh, tab activation, and the fallback
  // poll still call fetchRecentWorkflowRuns directly for an immediate refresh.
  let recentRunsRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let recentRunsRefreshing = false;
  let recentRunsRefreshQueued = false;

  async function runCoalescedRecentRunsRefresh() {
    if (recentRunsRefreshing) {
      recentRunsRefreshQueued = true;
      return;
    }

    recentRunsRefreshing = true;

    try {
      await fetchRecentWorkflowRuns({ background: true });
    } finally {
      recentRunsRefreshing = false;

      if (recentRunsRefreshQueued) {
        recentRunsRefreshQueued = false;
        scheduleRecentWorkflowRunsRefresh();
      }
    }
  }

  function scheduleRecentWorkflowRunsRefresh() {
    if (recentRunsRefreshTimer) {
      clearTimeout(recentRunsRefreshTimer);
    }

    recentRunsRefreshTimer = setTimeout(() => {
      recentRunsRefreshTimer = null;
      void runCoalescedRecentRunsRefresh();
    }, RECENT_RUNS_REFRESH_DEBOUNCE_MS);
  }

  // event-stream-driven detail refresh: trailing debounce + in-flight re-arm so a burst of
  // workflow_run_changed (drive + worker Running + chunks + terminal) collapses to one fetch of the
  // latest state. manual/user actions still call fetchWorkflowRunDetail directly.
  const detailRefreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const detailRefreshing = new Set<string>();
  const detailRefreshQueued = new Set<string>();

  async function runCoalescedDetailRefresh(runId: string) {
    if (detailRefreshing.has(runId)) {
      detailRefreshQueued.add(runId);
      return;
    }

    detailRefreshing.add(runId);

    try {
      await fetchWorkflowRunDetail(runId, true);
    } finally {
      detailRefreshing.delete(runId);

      if (detailRefreshQueued.has(runId)) {
        detailRefreshQueued.delete(runId);
        scheduleWorkflowRunDetailRefresh(runId);
      }
    }
  }

  function scheduleWorkflowRunDetailRefresh(runId: string) {
    if (!runId) {
      return;
    }

    const existing = detailRefreshTimers.get(runId);

    if (existing) {
      clearTimeout(existing);
    }

    detailRefreshTimers.set(
      runId,
      setTimeout(() => {
        detailRefreshTimers.delete(runId);
        void runCoalescedDetailRefresh(runId);
      }, WORKFLOW_RUN_DETAIL_REFRESH_DEBOUNCE_MS),
    );
  }

  async function selectWorkflowRun(run: RunSummary) {
    openRunInTab(run.id);
    activateRunTab(run.id);
    return fetchWorkflowRunDetail(run.id);
  }

  function openRunInTab(runId: string) {
    if (!runId) {
      return;
    }

    const ids = host.state.openRunIds;

    if (!ids.includes(runId)) {
      // Cap the tab count by evicting the oldest non-active tab.
      if (ids.length >= MAX_OPEN_RUN_TABS) {
        const victim = ids.find((id) => id !== host.state.selectedWorkflowRunId);

        if (victim) {
          closeRunTab(victim);
        }
      }

      host.state.openRunIds = [...ids, runId];
    }

    if (!internal.runDetailById.has(runId)) {
      internal.runDetailById.set(runId, null);
    }

    host.notify();
  }

  function activateRunTab(runId: string) {
    if (!runId) {
      return;
    }

    if (!host.state.openRunIds.includes(runId)) {
      openRunInTab(runId);
    }

    host.state.selectedWorkflowRunId = runId;
    const tabDetail = internal.runDetailById.get(runId) ?? null;
    host.state.workflowRunDetail = tabDetail;
    host.state.workflowNodeDetailExtra = "";
    host.state.selectedWorkflowRunNodeId = tabDetail?.nodes[0]?.node_id ?? "";

    if (tabDetail) {
      void syncWorkflowRunGatesForDetail(tabDetail);
    } else {
      clearWorkflowRunGates();
    }

    if (!internal.runDetailById.get(runId)) {
      void fetchWorkflowRunDetail(runId, true);
    }

    host.notify();
  }

  function closeRunTab(runId: string) {
    const ids = host.state.openRunIds;
    const index = ids.indexOf(runId);

    if (index === -1) {
      return;
    }

    const next = [...ids.slice(0, index), ...ids.slice(index + 1)];
    host.state.openRunIds = next;
    internal.runDetailById.delete(runId);
    internal.latestWorkflowRunPushVersion.delete(runId);
    internal.latestWorkflowRunHttpRequest.delete(runId);

    if (host.state.selectedWorkflowRunId === runId) {
      const replacement = next[Math.min(index, next.length - 1)] ?? null;

      if (replacement) {
        activateRunTab(replacement);
      } else {
        host.state.selectedWorkflowRunId = null;
        host.state.workflowRunDetail = null;
        host.state.selectedWorkflowRunNodeId = "";
        clearWorkflowRunGates();
      }
    }

    host.notify();
  }

  async function fetchWorkflowRunDetail(workflowRunId: string, silent = false) {
    console.info("[command-center] refreshing workflow run detail", { workflowRunId, silent });
    const requestStartedVersion = ++internal.nextWorkflowRunDetailVersion;
    const requestId = ++internal.nextWorkflowRunHttpRequestId;
    internal.latestWorkflowRunHttpRequest.set(workflowRunId, requestId);
    const detail = silent
      ? await fetchWorkflowRun(workflowRunId).catch(() => null)
      : await host.ctx
          .runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId))
          .catch(() => null);
    applyWorkflowRunDetail(detail, { source: "http", requestStartedVersion, requestId });
  }

  function setWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    if (detail) {
      internal.latestWorkflowRunPushVersion.set(
        detail.run.id,
        ++internal.nextWorkflowRunDetailVersion,
      );
    }

    applyWorkflowRunDetail(detail, { source: "ws" });
  }

  function selectWorkflowRunNode(nodeId: string) {
    host.state.selectedWorkflowRunNodeId = nodeId;
    void updateSelectedWorkflowNodeDetail();
    host.notify();
  }

  function clearWorkflowRunGates() {
    host.state.workflowRunGates = [];
    host.state.workflowRunGateRunId = null;
    host.state.workflowRunGateFingerprint = "";
    host.notify();
  }

  function workflowRunGateIds(detail: { nodes: { state?: JsonRecord }[] } | null): string[] {
    if (!detail) {
      return [];
    }

    const ids = detail.nodes
      .map((node) => node.state?.gate_id)
      .filter((value): value is string => typeof value === "string" && value.length > 0);
    return [...new Set(ids)].sort();
  }

  function workflowRunGateFingerprintForDetail(
    detail: { nodes: { state?: JsonRecord }[] } | null,
  ): string {
    return workflowRunGateIds(detail).join(",");
  }

  async function refreshWorkflowRunGates(runId: string, force = false) {
    const activeDetail =
      runId === host.state.workflowRunDetail?.run.id
        ? host.state.workflowRunDetail
        : (internal.runDetailById.get(runId) ?? null);
    const fingerprint = workflowRunGateFingerprintForDetail(activeDetail);

    if (
      !force &&
      host.state.workflowRunGateRunId === runId &&
      host.state.workflowRunGateFingerprint === fingerprint
    ) {
      return;
    }

    const requestId = ++internal.nextWorkflowRunGateRequestId;
    const gates = await fetchGates(runId).catch(() => null);

    if (requestId !== internal.nextWorkflowRunGateRequestId) {
      return;
    }

    if (
      host.state.selectedWorkflowRunId !== runId &&
      host.state.workflowRunDetail?.run.id !== runId
    ) {
      return;
    }

    host.state.workflowRunGates = asArray(gates).filter(isRecord) as unknown as GateRecord[];
    host.state.workflowRunGateRunId = runId;
    host.state.workflowRunGateFingerprint = fingerprint;
    host.notify();
  }

  async function syncWorkflowRunGatesForDetail(detail: WorkflowRunDetail | null, force = false) {
    if (!detail) {
      clearWorkflowRunGates();
      return;
    }

    await refreshWorkflowRunGates(detail.run.id, force);
  }

  async function resolveWorkflowRunGate(gateId: string, action: "open" | "close", reason?: string) {
    const runId = host.state.workflowRunDetail?.run.id ?? host.state.selectedWorkflowRunId;

    if (!runId) {
      host.ctx.setError("No workflow run selected");
      return;
    }

    const trimmed = reason?.trim() ? reason.trim() : undefined;
    const response = await host.ctx.runOperation(
      action === "open" ? "Opening gate" : "Closing gate",
      () => (action === "open" ? openGate(gateId, trimmed) : closeGate(gateId, trimmed)),
    );
    host.ctx.setStatus(response.message || `Gate ${action === "open" ? "opened" : "closed"}`);
    await Promise.all([fetchWorkflowRunDetail(runId, true), refreshWorkflowRunGates(runId, true)]);
    host.notify();
  }

  // keep the runs-list row in sync with a freshly streamed detail so the table reflects status,
  // timing, and output changes immediately, without waiting on a separate recent-runs refetch.
  function syncRunSummaryRow(run: RunSummary) {
    const index = host.state.workflowRuns.findIndex((entry) => entry.id === run.id);

    if (index === -1) {
      return;
    }

    const next = host.state.workflowRuns.slice();
    next[index] = {
      ...next[index],
      status: run.status,
      started_at: run.started_at,
      finished_at: run.finished_at,
      output_json: run.output_json,
      message: run.message,
      active_node_id: run.active_node_id,
    };
    host.state.workflowRuns = next;
  }

  function applyWorkflowRunDetail(
    detail: WorkflowRunDetail | null,
    metadata:
      { source: "http"; requestStartedVersion: number; requestId: number } | { source: "ws" } = {
      source: "ws",
    },
  ) {
    if (detail && metadata.source === "http") {
      const latestPushVersion = internal.latestWorkflowRunPushVersion.get(detail.run.id) ?? 0;
      const latestRequestId = internal.latestWorkflowRunHttpRequest.get(detail.run.id) ?? 0;

      if (
        latestPushVersion > metadata.requestStartedVersion ||
        latestRequestId !== metadata.requestId
      ) {
        console.info("[command-center] dropped stale workflow run detail", {
          runId: detail.run.id,
        });
        return;
      }
    }

    if (detail) {
      confirmPendingBreakpointPatch(detail);
    }

    if (detail) {
      internal.runDetailById.set(detail.run.id, detail);
      syncRunSummaryRow(detail.run);

      if (!host.state.openRunIds.includes(detail.run.id)) {
        host.state.openRunIds = [...host.state.openRunIds, detail.run.id].slice(-MAX_OPEN_RUN_TABS);
      }

      host.state.selectedWorkflowRunId ??= detail.run.id;
    }

    const isActiveRun = detail ? detail.run.id === host.state.selectedWorkflowRunId : true;

    if (isActiveRun) {
      host.state.workflowRunDetail = detail;
      reapplyPendingBreakpointPatch();
      host.state.workflowNodeDetailExtra = "";

      if (!detail?.nodes.some((node) => node.node_id === host.state.selectedWorkflowRunNodeId)) {
        host.state.selectedWorkflowRunNodeId = detail?.nodes[0]?.node_id ?? "";
      }

      if (detail) {
        void syncWorkflowRunGatesForDetail(detail);
      } else {
        clearWorkflowRunGates();
      }
    }

    if (detail) {
      const hasWaiting = detail.nodes.some(
        (n) => n.status === "waiting" || n.status === "approval_required" || n.status === "pending",
      );

      if (hasWaiting) {
        host.deps.refreshResources();
      }
    }

    host.notify();
  }

  function reapplyPendingBreakpointPatch() {
    if (!host.state.workflowRunDetail || !internal.pendingBreakpointPatch) {
      return;
    }

    if (host.state.workflowRunDetail.run.id !== internal.pendingBreakpointPatch.runId) {
      return;
    }

    applyBreakpointPatch(host.state.workflowRunDetail, internal.pendingBreakpointPatch.breakpoints);
    host.notify();
  }

  function confirmPendingBreakpointPatch(detail: WorkflowRunDetail) {
    const pending = internal.pendingBreakpointPatch;

    if (pending?.runId !== detail.run.id) {
      return;
    }

    if (sameBreakpoints(readBreakpoints(detail), pending.breakpoints)) {
      internal.pendingBreakpointPatch = null;
    }
  }

  function clearPendingBreakpointPatch(runId: string, mutationId: number) {
    if (
      internal.pendingBreakpointPatch?.runId === runId &&
      internal.pendingBreakpointPatch.mutationId === mutationId
    ) {
      internal.pendingBreakpointPatch = null;
      return true;
    }

    return false;
  }

  function applyBreakpointPatch(
    detail: WorkflowRunDetail | null,
    breakpoints: string[],
  ) {
    if (!detail) {
      return;
    }

    detail.execution_state ??= {};
    const debug = coerceDebugFrame(detail.execution_state.debug) ?? {};
    detail.execution_state.debug = { ...debug, breakpoints: [...breakpoints] };
  }

  function readBreakpoints(detail: WorkflowRunDetail): string[] {
    return coerceDebugFrame(detail.execution_state?.debug)?.breakpoints ?? [];
  }

  function sameBreakpoints(left: string[], right: string[]) {
    const normalizedLeft = [...new Set(left)].sort();
    const normalizedRight = [...new Set(right)].sort();

    if (normalizedLeft.length !== normalizedRight.length) {
      return false;
    }

    return normalizedLeft.every((id, index) => id === normalizedRight[index]);
  }

  async function updateSelectedWorkflowNodeDetail() {
    host.state.selectedWorkflowNodeRunId = null;
    host.state.workflowNodeDetailExtra = "";
    const nodeId = host.state.selectedWorkflowRunNodeId || host.state.selectedStepId;
    const step = host.state.workflowRunDetail?.nodes.find((node) => node.node_id === nodeId);

    if (!step?.id) {
      return;
    }

    host.state.selectedWorkflowNodeRunId = step.id;
    const [nodeChunks, nodeArtifacts] = await Promise.all([
      host.ctx
        .runOperation("Loading node chunks", () => fetchWorkflowNodeRunChunks(step.id))
        .catch(() => [] as RunChunk[]),
      host.ctx
        .runOperation("Loading node artifacts", () => fetchWorkflowNodeRunArtifacts(step.id))
        .catch(() => [] as RunArtifact[]),
    ]);
    host.state.workflowNodeDetailExtra = [
      "",
      `Workflow node run ${step.id} chunks`,
      ...nodeChunks.map((chunk) => `[${chunk.stream}] ${chunk.content}`),
      "",
      `Workflow node run ${step.id} artifacts`,
      ...nodeArtifacts.map(
        (artifact) => `${artifact.name} (${String(artifact.size_bytes)} bytes) ${artifact.uri}`,
      ),
    ].join("\n");
    host.notify();
  }

  // cancel many runs. terminal runs are dropped rather than sent — cancelling a finished run is a
  // guaranteed failure that would only pollute the outcome.
  async function cancelWorkflowRuns(runs: RunSummary[]) {
    const cancellable = runs.filter((run) => isActiveRunStatus(run.status));

    if (!cancellable.length) {
      host.ctx.setError("None of the selected runs are still active.");
      return;
    }

    const result = await host.ctx.runOperation(
      `Canceling ${String(cancellable.length)} workflow runs`,
      () =>
        runBulk(cancellable, async (run) => {
          const response = await cancelWorkflowRun(run.id);

          if (!response.success) {
            throw new Error(response.message || `Failed to cancel run ${run.id}`);
          }
        }),
    );

    await fetchRecentWorkflowRuns();

    if (host.state.selectedWorkflowRunId) {
      await fetchWorkflowRunDetail(host.state.selectedWorkflowRunId, true);
    }

    const text = describeBulkResult(result, "Canceled", "run");

    if (!result.failed.length) {
      host.ctx.setStatus(text);
    } else {
      // cancel is idempotent, so retrying only the failures is safe.
      const retryable = result.failed.map((failure) => failure.item);
      host.ctx.setError(text, {
        label: `Retry ${String(retryable.length)} failed`,
        run: () => {
          void cancelWorkflowRuns(retryable);
        },
      });
    }

    host.notify();
  }

  // replay many runs, each starting a fresh run from the beginning.
  //
  // deliberately offers no retry affordance: a replay creates a run, and a failure that surfaced
  // after the run was created would double-start it on retry. the user re-selects instead.
  async function replayWorkflowRuns(runs: RunSummary[]) {
    if (!runs.length) {
      return;
    }

    const result = await host.ctx.runOperation(
      `Replaying ${String(runs.length)} workflow runs`,
      () =>
        runBulk(
          runs,
          async (run) => {
            const created = await replayWorkflowRunApi(run.id, {});

            if (!created.id) {
              throw new Error(`Replay of run ${run.id} returned no run id`);
            }
          },
          // replays are new work, not a status flip: keep the fan-out narrow so a large selection
          // does not stampede the action queue.
          { concurrency: 2 },
        ),
    );

    await fetchRecentWorkflowRuns();
    const text = describeBulkResult(result, "Replayed", "run");

    if (result.failed.length) {
      host.ctx.setError(text);
    } else {
      host.ctx.setStatus(text);
    }

    host.ctx.activeTab = "Runs";
    host.notify();
  }

  return {
    ...watchService,
    isBreakpointed,
    getTransition,
    setTransition,
    runSelectedWorkflow,
    runSelectedWorkflowDebug,
    closeRunInput,
    confirmRunInput,
    launchWorkflowRun,
    stepSelectedWorkflowRun,
    continueSelectedWorkflowRun,
    selectCursor,
    forkCursor,
    retireCursor,
    armNodeForReal,
    cancelSelectedWorkflowRun,
    requestSelectedRunInterrupt,
    pauseSelectedWorkflowRun,
    resumeSelectedWorkflowRun,
    patchSelectedWorkflowRunDebug,
    toggleBreakpoint,
    runToCursor,
    skipCurrentNode,
    rerunCurrentNode,
    replaySelectedWorkflowRun,
    renameSelectedWorkflowRun,
    cancelWorkflowRuns,
    replayWorkflowRuns,
    fetchWorkflowRunsForSelected,
    fetchRecentWorkflowRuns,
    scheduleRecentWorkflowRunsRefresh,
    scheduleWorkflowRunDetailRefresh,
    selectWorkflowRun,
    openRunInTab,
    activateRunTab,
    closeRunTab,
    fetchWorkflowRunDetail,
    setWorkflowRunDetail,
    selectWorkflowRunNode,
    clearWorkflowRunGates,
    workflowRunGateIds,
    workflowRunGateFingerprintForDetail,
    refreshWorkflowRunGates,
    syncWorkflowRunGatesForDetail,
    resolveWorkflowRunGate,
    applyWorkflowRunDetail,
    reapplyPendingBreakpointPatch,
    confirmPendingBreakpointPatch,
    clearPendingBreakpointPatch,
    applyBreakpointPatch,
    readBreakpoints,
    sameBreakpoints,
    updateSelectedWorkflowNodeDetail,
  };
}
