// running one action over a selection of rows. a bulk action is many independent calls, not one
// transaction — the backend has no batch endpoints — so partial failure is the normal outcome and
// the result has to carry enough detail to report and retry exactly the items that failed.

export interface BulkFailure<T> {
  item: T;
  error: unknown;
  message: string;
}

export interface BulkResult<T> {
  succeeded: T[];
  failed: BulkFailure<T>[];
  // true when every item failed; the caller reports this as an outright failure rather than "partial".
  allFailed: boolean;
}

const DEFAULT_CONCURRENCY = 4;

function messageFor(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

/// run `operation` over every item with bounded concurrency, collecting rather than propagating
/// failures. never rejects: a caller wants the partial outcome, not the first error.
export async function runBulk<T>(
  items: readonly T[],
  operation: (item: T) => Promise<unknown>,
  options: { concurrency?: number; signal?: AbortSignal } = {},
): Promise<BulkResult<T>> {
  const concurrency = Math.max(1, options.concurrency ?? DEFAULT_CONCURRENCY);
  const succeeded: T[] = [];
  const failed: BulkFailure<T>[] = [];
  let cursor = 0;

  async function worker() {
    while (cursor < items.length) {
      if (options.signal?.aborted) {
        return;
      }

      const index = cursor;
      cursor += 1;
      const item = items[index];

      try {
        await operation(item);
        succeeded.push(item);
      } catch (error) {
        failed.push({ item, error, message: messageFor(error) });
      }
    }
  }

  const lanes = Math.min(concurrency, items.length);
  await Promise.all(Array.from({ length: lanes }, () => worker()));

  return { succeeded, failed, allFailed: items.length > 0 && succeeded.length === 0 };
}

/// one-line outcome for a status/toast line, e.g. "Disabled 8 of 10 workflows (2 failed: ...)".
/// `verb` is past tense. callers pick the tone from `result.allFailed` rather than from this text.
export function describeBulkResult<T>(result: BulkResult<T>, verb: string, noun: string): string {
  const total = result.succeeded.length + result.failed.length;
  const plural = total === 1 ? noun : `${noun}s`;

  if (!result.failed.length) {
    return `${verb} ${String(total)} ${plural}`;
  }

  const reason = result.failed[0]?.message ?? "unknown error";
  return `${verb} ${String(result.succeeded.length)} of ${String(total)} ${plural} (${String(result.failed.length)} failed: ${reason})`;
}
