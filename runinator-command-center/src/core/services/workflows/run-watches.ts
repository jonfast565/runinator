import type { WorkflowServiceHost } from "./host";

const WATCH_STORAGE_PREFIX = "runinator.watch.";

export function createWorkflowRunWatchService(host: WorkflowServiceHost) {
  function loadAllWatchExpressions(): Record<string, string[]> {
    const storage = typeof window !== "undefined" ? window.localStorage : undefined;

    if (!storage) {
      return {};
    }

    const result: Record<string, string[]> = {};

    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);

      if (!key?.startsWith(WATCH_STORAGE_PREFIX)) {
        continue;
      }

      const id = key.slice(WATCH_STORAGE_PREFIX.length);

      if (!id) {
        continue;
      }

      try {
        const parsed: unknown = JSON.parse(storage.getItem(key) ?? "[]");

        if (Array.isArray(parsed)) {
          result[id] = parsed.filter((value): value is string => typeof value === "string");
        }
      } catch {
        // malformed local state is ignored.
      }
    }

    return result;
  }

  function persistWatchExpressions(workflowId: string, list: string[]) {
    const storage = typeof window !== "undefined" ? window.localStorage : undefined;
    storage?.setItem(`${WATCH_STORAGE_PREFIX}${workflowId}`, JSON.stringify(list));
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
