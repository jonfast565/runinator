// the workflow *header*: interrupt handlers, watch guards, the concurrency policy, and the
// correlation key.
//
// these are the declarations that belong to the workflow rather than to any node, and until now
// they could only be written by typing wdl. every mutator here follows the same contract as a node
// edit -- change `state.headerDraft`, write it into the definition, then `syncWorkflowDraftToJson()`
// -- which is what keeps the json pane, the wdl pane, the diagnostics table, and the dirty flag in
// step. components never touch `definition.metadata` themselves.

import type { JsonRecord, JsonValue, WorkflowValidationIssue } from "../../domain/models";
import {
  applyWorkflowHeader as writeWorkflowHeader,
  readWorkflowHeader,
} from "../../workflow/header-metadata";
import type { ConcurrencyHeader, WatchDeclaration } from "../../workflow/header-metadata";
import {
  HEADER_ISSUE_NODE_ID,
  declarationIssues,
  headerIssues,
  interruptIssues,
} from "../../workflow/header-validation";
import { interruptRegionNodes, nodesById } from "../../workflow/interrupt-regions";
import { findNodeKindMetadata, isNodeCatalogLoaded } from "../../workflow/catalog-registry";
import { createWorkflowNode, uniqueWorkflowNodeId } from "../../workflow/index";
import { displayValue } from "../../utils/values";
import type { WorkflowServiceHost } from "./host";

/** what the header service needs back from the editor: the one write path into the draft. */
export interface WorkflowHeaderPeer {
  syncWorkflowDraftToJson(): void;
  setGraphNodePosition(nodeId: string, position: { x: number; y: number }): void;
  populateStepEditor(nodeId: string): void;
  openStepEditor(nodeId: string, creating?: boolean): void;
  ensureWorkflowNodes(): JsonRecord[];
  removeWorkflowNodes(nodeIds: string[]): void;
}

/** horizontal/vertical spacing of a scaffolded region, matching `newWorkflowDraft`'s layout. */
const SCAFFOLD_STEP_X = 270;
const SCAFFOLD_GAP_Y = 180;

export function createWorkflowHeaderService(host: WorkflowServiceHost, editor: WorkflowHeaderPeer) {
  function definition(): JsonRecord {
    return host.state.workflowDraft.definition;
  }

  /** re-read the working copy from the draft. call whenever the definition changed elsewhere. */
  function populateWorkflowHeader() {
    host.state.headerDraft = readWorkflowHeader(definition());
  }

  function openWorkflowHeader() {
    populateWorkflowHeader();
    host.state.workflowInspectorMode = "header";
    host.notify();
  }

  /** interrupts read and write the same draft as the header; only the panel they show in differs. */
  function openWorkflowInterrupts() {
    populateWorkflowHeader();
    host.state.workflowInspectorMode = "interrupts";
    host.notify();
  }

  function openWorkflowWdl() {
    host.state.workflowInspectorMode = "wdl";
    host.notify();
  }

  function closeWorkflowHeader() {
    host.state.workflowInspectorMode = "step";
    host.notify();
  }

  /** the single write point: draft -> definition -> json/wdl panes. */
  function applyWorkflowHeader() {
    writeWorkflowHeader(definition(), host.state.headerDraft);
    editor.syncWorkflowDraftToJson();
    host.notify();
  }

  // -- interrupts ------------------------------------------------------------------------------

  /**
   * repoint a declaration at a different source. the link lives in metadata; the graph entry stays
   * a source-neutral structural boundary around the handler region.
   */
  function setHeaderInterruptSource(index: number, source: string) {
    const entry = host.state.headerDraft.interrupts.at(index);

    if (!entry) {
      return;
    }

    entry.source = source;
    applyWorkflowHeader();
  }

  function setHeaderInterruptEnabled(index: number, enabled: boolean) {
    const entry = host.state.headerDraft.interrupts.at(index);

    if (!entry) {
      return;
    }

    entry.enabled = enabled;
    applyWorkflowHeader();
  }

  /**
   * delete a handler region after confirmation.
   *
   * metadata owns the link, but an unlinked region cannot be represented in wdl. disabling is the
   * non-destructive option; deletion removes the link and its bounded graph region together.
   */
  function removeHeaderInterrupt(index: number) {
    const entry = host.state.headerDraft.interrupts.at(index);
    // a dangling target (already reported as a validation error) is not a real node to offer
    // deleting, so it is excluded rather than prompting to "delete" something that does not exist.
    const walk = entry
      ? interruptRegionNodes(nodesById(host.state.workflowDraft), entry.handler)
      : null;
    const regionNodeIds = walk ? [...walk.nodes].filter((id) => !walk.missing.has(id)) : [];

    if (regionNodeIds.length === 0) {
      host.state.headerDraft.interrupts.splice(index, 1);
      writeWorkflowHeader(definition(), host.state.headerDraft);
      editor.syncWorkflowDraftToJson();
      host.notify();
      return;
    }

    const prompt =
      regionNodeIds.length === 1
        ? `Delete interrupt handler '${entry?.source ?? ""}' and its region node '${regionNodeIds[0]}'?`
        : `Delete interrupt handler '${entry?.source ?? ""}' and its ${String(regionNodeIds.length)} region nodes (${regionNodeIds.join(", ")})?`;

    if (!host.deps.confirm(prompt)) {
      return;
    }

    host.state.headerDraft.interrupts.splice(index, 1);
    writeWorkflowHeader(definition(), host.state.headerDraft);
    editor.removeWorkflowNodes(regionNodeIds);
    populateWorkflowHeader();

    host.ctx.setStatus(`Deleted interrupt handler and ${String(regionNodeIds.length)} region node(s)`);
  }

  // -- watches ---------------------------------------------------------------------------------

  function addHeaderWatch() {
    host.state.headerDraft.watches.push({ condition: { value: true }, handler: "end" });
    applyWorkflowHeader();
  }

  function setHeaderWatch(index: number, patch: Partial<WatchDeclaration>) {
    const entry = host.state.headerDraft.watches.at(index);

    if (!entry) {
      return;
    }

    Object.assign(entry, patch);
    applyWorkflowHeader();
  }

  function removeHeaderWatch(index: number) {
    host.state.headerDraft.watches.splice(index, 1);
    applyWorkflowHeader();
  }

  // -- concurrency and correlation --------------------------------------------------------------

  function setHeaderConcurrency(patch: Partial<ConcurrencyHeader>) {
    const current: ConcurrencyHeader = host.state.headerDraft.concurrency ?? {
      maxConcurrentRuns: 1,
      onConflict: "skip",
    };
    host.state.headerDraft.concurrency = { ...current, ...patch };
    applyWorkflowHeader();
  }

  function clearHeaderConcurrency() {
    host.state.headerDraft.concurrency = null;
    applyWorkflowHeader();
  }

  function setHeaderCorrelation(expression: JsonValue | null) {
    host.state.headerDraft.correlation = expression;
    applyWorkflowHeader();
  }

  // -- panel queries ----------------------------------------------------------------------------

  /** the header's own diagnostics, without the per-node issues the canvas table also shows. */
  function getHeaderIssues(): WorkflowValidationIssue[] {
    return headerIssues(definition());
  }

  function getHeaderIssueCount(): number {
    return errorCount(getHeaderIssues());
  }

  /** the interrupts panel's diagnostics. the header panel shows `getDeclarationIssues()` instead. */
  function getInterruptIssues(): WorkflowValidationIssue[] {
    return interruptIssues(definition());
  }

  function getInterruptIssueCount(): number {
    return errorCount(getInterruptIssues());
  }

  /** the header panel's diagnostics: everything except the interrupts, which have their own panel. */
  function getDeclarationIssues(): WorkflowValidationIssue[] {
    return declarationIssues(definition());
  }

  function getDeclarationIssueCount(): number {
    return errorCount(getDeclarationIssues());
  }

  function errorCount(issues: WorkflowValidationIssue[]): number {
    return issues.filter((issue) => issue.severity === "error").length;
  }

  /** every node in the region entered at `handler`, for the panel's region chip list. */
  function getRegionNodeIds(handler: string): string[] {
    return [...interruptRegionNodes(nodesById(host.state.workflowDraft), handler).nodes];
  }

  /** the sources that do not have a handler yet, in catalog order. */
  function getUndeclaredInterruptSources(sources: string[]): string[] {
    const declared = new Set(host.state.headerDraft.interrupts.map((entry) => entry.source));
    return sources.filter((source) => !declared.has(source));
  }

  // -- scaffolding -------------------------------------------------------------------------------

  /**
   * create a minimal, valid handler region for `source` and declare it.
   *
   * the region is an `interrupt -> audit -> resume` sequence: metadata links the source to the
   * source-neutral entry, the audit is immediately editable, and resume hands control back.
   * authors can change or extend that middle step without first breaking a structural edge.
   *
   * the resulting region satisfies every rule in `validate_interrupt_handlers` by construction --
   * nothing can enter it (an `interrupt` is an entry point, so no edge may target it), both kinds
   * are handler-safe, it ends at a resume, and no node is reached twice -- which the
   * header-validation suite pins.
   */
  function scaffoldInterruptHandler(source: string): boolean {
    if (
      !isNodeCatalogLoaded() ||
      !findNodeKindMetadata("resume") ||
      !findNodeKindMetadata("interrupt") ||
      !findNodeKindMetadata("audit")
    ) {
      host.ctx.setError("Node types are still loading; try again in a moment");
      return false;
    }

    if (host.state.headerDraft.interrupts.some((entry) => entry.source === source)) {
      host.ctx.setError(`Source '${source}' already has a handler; one handler per source`);
      return false;
    }

    const nodes = editor.ensureWorkflowNodes();
    const entry = createWorkflowNode("interrupt", nodes);
    entry.id = uniqueWorkflowNodeId(nodes, `on_${source}`);
    insertBeforeEnd(nodes, entry);

    // give the author a real, editable step between the structural endpoints. asking them to add a
    // node, delete the direct edge, and reconnect both halves is needless graph surgery for the
    // most common next action after creating a handler.
    const body = createWorkflowNode("audit", nodes);
    body.id = uniqueWorkflowNodeId(nodes, `handle_${source}`);
    body.parameters = { action: `interrupt:${source}` };
    insertBeforeEnd(nodes, body);

    // claimed after the other nodes are in the list so all three ids are distinct.
    const resume = createWorkflowNode("resume", nodes);
    resume.id = uniqueWorkflowNodeId(nodes, `resume_${source}`);
    insertBeforeEnd(nodes, resume);

    entry.transitions = { next: { $node: body.id } };
    body.transitions = { next: { $node: resume.id } };

    const origin = scaffoldOrigin();
    editor.setGraphNodePosition(displayValue(entry.id), origin);
    editor.setGraphNodePosition(displayValue(body.id), {
      x: origin.x + SCAFFOLD_STEP_X,
      y: origin.y,
    });
    editor.setGraphNodePosition(displayValue(resume.id), {
      x: origin.x + SCAFFOLD_STEP_X * 2,
      y: origin.y,
    });

    host.state.headerDraft.interrupts.push({
      source,
      handler: displayValue(entry.id),
      enabled: true,
    });
    applyWorkflowHeader();
    host.ctx.setStatus(`Added an interrupt handler for '${source}'`);
    editor.openStepEditor(displayValue(body.id));
    return true;
  }

  function insertBeforeEnd(nodes: JsonRecord[], node: JsonRecord) {
    const endIndex = nodes.findIndex((entry) => entry.kind === "end");

    if (endIndex >= 0) {
      nodes.splice(endIndex, 0, node);
      return;
    }

    nodes.push(node);
  }

  /**
   * where to drop a new region: below the existing graph, not on top of it.
   *
   * the canvas centroid would land the island in the middle of the main flow. auto-arrange already
   * handles disconnected components, so a later re-layout keeps this readable.
   */
  function scaffoldOrigin(): { x: number; y: number } {
    const positions = host.buildDraftGraphNodes().map((node) => node.position);

    if (positions.length === 0) {
      return { x: 0, y: 0 };
    }

    return {
      x: Math.min(...positions.map((position) => position.x)),
      y: Math.max(...positions.map((position) => position.y)) + SCAFFOLD_GAP_Y,
    };
  }

  return {
    HEADER_ISSUE_NODE_ID,
    populateWorkflowHeader,
    openWorkflowHeader,
    openWorkflowInterrupts,
    openWorkflowWdl,
    closeWorkflowHeader,
    applyWorkflowHeader,
    setHeaderInterruptSource,
    setHeaderInterruptEnabled,
    removeHeaderInterrupt,
    scaffoldInterruptHandler,
    addHeaderWatch,
    setHeaderWatch,
    removeHeaderWatch,
    setHeaderConcurrency,
    clearHeaderConcurrency,
    setHeaderCorrelation,
    getHeaderIssues,
    getHeaderIssueCount,
    getInterruptIssues,
    getInterruptIssueCount,
    getDeclarationIssues,
    getDeclarationIssueCount,
    getRegionNodeIds,
    getUndeclaredInterruptSources,
  };
}
