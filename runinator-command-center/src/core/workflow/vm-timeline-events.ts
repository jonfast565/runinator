import { asJsonRecord } from "../domain/json";
import type { JsonRecord, WorkflowJournalRecord, WorkflowNodeRun } from "../domain/models";

interface JournalNodeEntry {
  nodeId: string;
  journalEntryId: string;
}

export interface JournalFailure {
  record: WorkflowJournalRecord;
  continuationId: string;
  nodeId: string;
  message: string;
}

/** match an inline VM failure to one exact node visit, preserving loop and branch identity. */
export function journalFailures(journal: WorkflowJournalRecord[]): {
  byNodeEntryId: Map<string, JournalFailure>;
  unmatched: JournalFailure[];
} {
  const lastNodeByContinuation = new Map<string, JournalNodeEntry>();
  const byNodeEntryId = new Map<string, JournalFailure>();
  const unmatched: JournalFailure[] = [];

  for (const record of [...journal].sort((left, right) => left.sequence - right.sequence)) {
    const entry = asJsonRecord(record.entry);
    const continuationId =
      record.continuation_id ??
      (typeof entry.continuation_id === "string" ? entry.continuation_id : null);

    if (entry.type === "node_entered" && continuationId && typeof entry.node_id === "string") {
      lastNodeByContinuation.set(continuationId, {
        nodeId: entry.node_id,
        journalEntryId: record.id,
      });
      continue;
    }

    if (entry.type !== "failed" || !continuationId || typeof entry.node_id !== "string") {
      continue;
    }

    const failure = {
      record,
      continuationId,
      nodeId: entry.node_id,
      message: typeof entry.message === "string" ? entry.message : "Workflow node failed.",
    };
    const node = lastNodeByContinuation.get(continuationId);

    if (node?.nodeId === failure.nodeId) {
      byNodeEntryId.set(node.journalEntryId, failure);
    } else {
      unmatched.push(failure);
    }
  }

  return { byNodeEntryId, unmatched };
}

/** project graph-relevant VM mechanics as instant rows anchored to an authored node. */
export function vmLifecycleNodes(journal: WorkflowJournalRecord[]): WorkflowNodeRun[] {
  const lastNodeByContinuation = new Map<string, string>();
  const nodes: WorkflowNodeRun[] = [];

  for (const record of [...journal].sort((left, right) => left.sequence - right.sequence)) {
    const entry = asJsonRecord(record.entry);
    const recordContinuationId =
      record.continuation_id ??
      (typeof entry.continuation_id === "string" ? entry.continuation_id : null);

    if (
      entry.type === "node_entered" &&
      recordContinuationId &&
      typeof entry.node_id === "string"
    ) {
      lastNodeByContinuation.set(recordContinuationId, entry.node_id);
      continue;
    }

    let continuationId: string | null = null;
    let nodeId: string | undefined;
    let status = "succeeded";
    let message: string | null = null;
    let parameters: JsonRecord = {};

    if (entry.type === "forked" && typeof entry.continuation_id === "string") {
      continuationId = entry.continuation_id;
      nodeId = lastNodeByContinuation.get(continuationId);
      const children = Array.isArray(entry.children)
        ? entry.children.filter((child): child is string => typeof child === "string")
        : [];
      const joinKey = typeof entry.join_key === "string" ? entry.join_key : "branch";
      status = "forked";
      message = `Forked ${String(children.length)} branch${children.length === 1 ? "" : "es"} for ${joinKey}.`;
      parameters = { join_key: joinKey, children };
    } else if (entry.type === "interrupted" && typeof entry.continuation_id === "string") {
      continuationId = entry.continuation_id;
      nodeId = lastNodeByContinuation.get(continuationId);
      const source = typeof entry.source === "string" ? entry.source : "unknown";
      const handlerId =
        typeof entry.handler_continuation_id === "string" ? entry.handler_continuation_id : null;
      status = "interrupted";
      message = handlerId
        ? `${source.replaceAll("_", " ")} interrupt started handler ${handlerId.slice(0, 8)}.`
        : `${source.replaceAll("_", " ")} interrupt started.`;
      parameters = { source, handler_continuation_id: handlerId };
    } else if (
      entry.type === "interrupt_resolved" &&
      typeof entry.handler_continuation_id === "string"
    ) {
      continuationId = entry.handler_continuation_id;
      nodeId = lastNodeByContinuation.get(continuationId);
      const outcome = asJsonRecord(entry.outcome);
      const outcomeType = typeof outcome.type === "string" ? outcome.type : "resolved";
      status = "resolved";
      message =
        outcomeType === "fail" && typeof outcome.message === "string"
          ? `Interrupt handler resolved with failure: ${outcome.message}`
          : `Interrupt handler resolved with ${outcomeType.replaceAll("_", " ")}.`;
      parameters = { outcome };
    }

    if (!continuationId || !nodeId || !message) {
      continue;
    }

    const timestamp = new Date(record.created_at * 1000).toISOString();
    nodes.push({
      id: record.id,
      workflow_run_id: record.workflow_run_id,
      node_id: nodeId,
      status,
      attempt: 0,
      parameters,
      timeline_category: record.timeline_category ?? "system",
      state: {
        journal_entry_id: record.id,
        vm_event_type: entry.type,
        timeline_sequence: record.sequence,
      },
      cursor_id: continuationId,
      created_at: timestamp,
      started_at: timestamp,
      finished_at: timestamp,
      message,
    });
  }

  return nodes;
}
