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
import { HEADER_ISSUE_NODE_ID, headerIssues } from "../../workflow/header-validation";
import { interruptRegionNodes, nodesById } from "../../workflow/interrupt-regions";
import { findNodeKindMetadata, isNodeCatalogLoaded } from "../../workflow/catalog-registry";
import { createWorkflowNode, uniqueWorkflowNodeId } from "../../workflow/index";
import { displayValue } from "../../utils/values";
import type { WorkflowServiceHost } from "./host";

/** what the header service needs back from the editor: the one write path into the draft. */
export interface WorkflowHeaderPeer {
  syncWorkflowDraftToJson(): void;
  stripNewNodeConnections(node: JsonRecord): void;
  setGraphNodePosition(nodeId: string, position: { x: number; y: number }): void;
  populateStepEditor(nodeId: string): void;
  ensureWorkflowNodes(): JsonRecord[];
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

  function declareHeaderInterrupt(source: string, handler: string) {
    host.state.headerDraft.interrupts.push({ source, handler });
    applyWorkflowHeader();
  }

  function setHeaderInterruptSource(index: number, source: string) {
    const entry = host.state.headerDraft.interrupts[index];

    if (!entry) {
      return;
    }

    entry.source = source;
    applyWorkflowHeader();
  }

  function setHeaderInterruptHandler(index: number, handler: string) {
    const entry = host.state.headerDraft.interrupts[index];

    if (!entry) {
      return;
    }

    entry.handler = handler;
    applyWorkflowHeader();
  }

  function removeHeaderInterrupt(index: number) {
    host.state.headerDraft.interrupts.splice(index, 1);
    applyWorkflowHeader();
  }

  // -- watches ---------------------------------------------------------------------------------

  function addHeaderWatch() {
    host.state.headerDraft.watches.push({ condition: { value: true }, handler: "end" });
    applyWorkflowHeader();
  }

  function setHeaderWatch(index: number, patch: Partial<WatchDeclaration>) {
    const entry = host.state.headerDraft.watches[index];

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
    return getHeaderIssues().filter((issue) => issue.severity === "error").length;
  }

  /**
   * nodes a handler may be pointed at: enterable, and not already part of the main flow.
   *
   * the reachability half is the rule that actually bites -- a region must be entered only by its
   * interrupt -- so filtering here means the picker cannot offer a choice the validator rejects.
   */
  function getHandlerCandidateNodeIds(): string[] {
    const byId = nodesById(host.state.workflowDraft);
    const start = displayValue(definition().start);
    const reachable = start ? interruptRegionNodes(byId, start).nodes : new Set<string>();

    return [...byId]
      .filter(([id, node]) => {
        if (reachable.has(id)) {
          return false;
        }

        const metadata = findNodeKindMetadata(displayValue(node.kind));
        return metadata ? metadata.runnable_entry : true;
      })
      .map(([id]) => id);
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
   * two nodes rather than one: a bare `resume` is a legal region but leaves nowhere to put the
   * handler's actual work, and adding a node into a disconnected island through the canvas is
   * fiddly. `audit` is the smallest handler-safe kind that records something useful.
   *
   * the resulting region satisfies every rule in `validate_interrupt_handlers` by construction --
   * nothing enters it, both kinds are handler-safe, it ends at a resume, and no node is reached
   * twice -- which the header-validation suite pins.
   */
  function scaffoldInterruptHandler(source: string): boolean {
    if (!isNodeCatalogLoaded() || !findNodeKindMetadata("resume")) {
      host.ctx.setError("Node types are still loading; try again in a moment");
      return false;
    }

    if (host.state.headerDraft.interrupts.some((entry) => entry.source === source)) {
      host.ctx.setError(`Source '${source}' already has a handler; one handler per source`);
      return false;
    }

    const nodes = editor.ensureWorkflowNodes();
    const entry = createWorkflowNode("audit", nodes);
    entry.id = uniqueWorkflowNodeId(nodes, `on_${source}`);
    // the audit template points at `end`. leaving that would drag `end` into the region, and `end`
    // is not handler-safe -- the region would be rejected the moment it was declared.
    editor.stripNewNodeConnections(entry);
    entry.parameters = { action: `interrupt:${source}` };
    insertBeforeEnd(nodes, entry);

    // claimed after the entry node is in the list so the two ids cannot collide.
    const resume = createWorkflowNode("resume", nodes);
    resume.id = uniqueWorkflowNodeId(nodes, `resume_${source}`);
    insertBeforeEnd(nodes, resume);

    entry.transitions = { next: { $node: resume.id } };

    const origin = scaffoldOrigin();
    editor.setGraphNodePosition(displayValue(entry.id), origin);
    editor.setGraphNodePosition(displayValue(resume.id), {
      x: origin.x + SCAFFOLD_STEP_X,
      y: origin.y,
    });

    declareHeaderInterrupt(source, displayValue(entry.id));
    editor.populateStepEditor(displayValue(entry.id));
    host.ctx.setStatus(`Added an interrupt handler for '${source}'`);
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
    closeWorkflowHeader,
    applyWorkflowHeader,
    declareHeaderInterrupt,
    setHeaderInterruptSource,
    setHeaderInterruptHandler,
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
    getHandlerCandidateNodeIds,
    getRegionNodeIds,
    getUndeclaredInterruptSources,
  };
}
