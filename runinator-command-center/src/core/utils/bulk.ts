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

function messageFor(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

class BulkOperation<T> {
  private static readonly DEFAULT_CONCURRENCY = 4;

  private readonly succeeded: T[] = [];
  private readonly failed: BulkFailure<T>[] = [];
  private readonly concurrency: number;
  private cursor = 0;

  constructor(
    private readonly items: readonly T[],
    private readonly operation: (item: T) => Promise<unknown>,
    options: { concurrency?: number; signal?: AbortSignal },
  ) {
    this.concurrency = Math.max(
      1,
      options.concurrency ?? BulkOperation.DEFAULT_CONCURRENCY,
    );
    this.signal = options.signal;
  }

  private readonly signal: AbortSignal | undefined;

  async run(): Promise<BulkResult<T>> {
    const lanes = Math.min(this.concurrency, this.items.length);
    await Promise.all(Array.from({ length: lanes }, () => this.runLane()));

    return {
      succeeded: this.succeeded,
      failed: this.failed,
      allFailed: this.items.length > 0 && this.succeeded.length === 0,
    };
  }

  private async runLane() {
    while (this.cursor < this.items.length) {
      if (this.signal?.aborted) {
        return;
      }

      const item = this.items[this.cursor];
      this.cursor += 1;

      try {
        await this.operation(item);
        this.succeeded.push(item);
      } catch (error) {
        this.failed.push({ item, error, message: messageFor(error) });
      }
    }
  }
}

// run `operation` over every item with bounded concurrency, collecting rather than propagating
// failures. never rejects: a caller wants the partial outcome, not the first error.
export async function runBulk<T>(
  items: readonly T[],
  operation: (item: T) => Promise<unknown>,
  options: { concurrency?: number; signal?: AbortSignal } = {},
): Promise<BulkResult<T>> {
  return new BulkOperation(items, operation, options).run();
}

// one-line outcome for a status/toast line, e.g. "Disabled 8 of 10 workflows (2 failed: ...)".
// `verb` is past tense. callers pick the tone from `result.allFailed` rather than from this text.
export function describeBulkResult<T>(result: BulkResult<T>, verb: string, noun: string): string {
  const total = result.succeeded.length + result.failed.length;
  const plural = total === 1 ? noun : `${noun}s`;

  if (!result.failed.length) {
    return `${verb} ${String(total)} ${plural}`;
  }

  const reason = result.failed[0]?.message ?? "unknown error";
  return `${verb} ${String(result.succeeded.length)} of ${String(total)} ${plural} (${String(result.failed.length)} failed: ${reason})`;
}
