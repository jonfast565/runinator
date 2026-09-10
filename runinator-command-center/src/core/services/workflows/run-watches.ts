import { browserPreferences, type PreferenceStorage } from "../preference-storage";
import type { WorkflowServiceHost } from "./host";

const WATCH_STORAGE_PREFIX = "runinator.watch.";

export interface WatchExpressionRepository {
  loadAll(): Record<string, string[]>;
  save(workflowId: string, expressions: readonly string[]): void;
}

export class WatchExpressionStorage implements WatchExpressionRepository {
  constructor(
    private readonly storage: PreferenceStorage = browserPreferences,
    private readonly prefix = WATCH_STORAGE_PREFIX,
  ) {}

  loadAll(): Record<string, string[]> {
    const result: Record<string, string[]> = {};

    for (const key of this.storage.keys()) {
      if (!key.startsWith(this.prefix)) {
        continue;
      }

      const id = key.slice(this.prefix.length);

      if (!id) {
        continue;
      }

      try {
        const parsed: unknown = JSON.parse(this.storage.getItem(key) ?? "[]");

        if (Array.isArray(parsed)) {
          result[id] = parsed.filter((value): value is string => typeof value === "string");
        }
      } catch {
        // malformed local state is ignored.
      }
    }

    return result;
  }

  save(workflowId: string, expressions: readonly string[]) {
    this.storage.setItem(`${this.prefix}${workflowId}`, JSON.stringify(expressions));
  }
}

export function createWorkflowRunWatchService(
  host: WorkflowServiceHost,
  storage: WatchExpressionRepository = new WatchExpressionStorage(),
) {
  function loadAllWatchExpressions(): Record<string, string[]> {
    return storage.loadAll();
  }

  function persistWatchExpressions(workflowId: string, list: string[]) {
    storage.save(workflowId, list);
  }

  function addWatchExpression(expression: string) {
    const workflowId = host.getWorkflowRunWorkflow()?.id;

    if (!workflowId || !expression.trim()) {
      return;
    }

    const existing = host.state.watchExpressionsByWorkflowId[workflowId] ?? [];

    if (existing.includes(expression)) {
      return;
    }

    const next = [...existing, expression];
    host.state.watchExpressionsByWorkflowId = {
      ...host.state.watchExpressionsByWorkflowId,
      [workflowId]: next,
    };
    persistWatchExpressions(workflowId, next);
    host.notify();
  }

  function removeWatchExpression(expression: string) {
    const workflowId = host.getWorkflowRunWorkflow()?.id;

    if (!workflowId) {
      return;
    }

    const next = (host.state.watchExpressionsByWorkflowId[workflowId] ?? []).filter(
      (entry) => entry !== expression,
    );
    host.state.watchExpressionsByWorkflowId = {
      ...host.state.watchExpressionsByWorkflowId,
      [workflowId]: next,
    };
    persistWatchExpressions(workflowId, next);
    host.notify();
  }

  return {
    loadAllWatchExpressions,
    persistWatchExpressions,
    addWatchExpression,
    removeWatchExpression,
  };
}
