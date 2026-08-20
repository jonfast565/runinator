//! the node retry policy, as the editor needs to show it.
//!
//! the delay schedule mirrors `retry_backoff_delay` in `runinator-runtime`'s `transitions.rs`:
//! `clamp(base * 2^(attempt - 1), base, max)`, where `attempt` is the number of the attempt that
//! just failed — so a node with `max_attempts: N` waits out N-1 delays computed from attempts
//! 1..N-1. Exponential backoff with a cap is hard to hold in your head, which is the whole reason
//! the editor previews it; a preview that disagreed with the runtime would be worse than none.

export interface RetryPolicy {
  max_attempts: number;
  backoff_base_seconds: number;
  backoff_max_seconds: number;
  jitter: boolean;
  retry_on: string;
}

export const RETRY_CLASSES: { value: string; label: string; description: string }[] = [
  {
    value: "any",
    label: "Failure or timeout",
    description: "Retry both a failed run and one that blew its deadline.",
  },
  {
    value: "failure",
    label: "Failure only",
    description: "A timeout falls straight through to its transition instead of retrying.",
  },
  {
    value: "timeout",
    label: "Timeout only",
    description: "An outright failure falls through; only a blown deadline is retried.",
  },
];

/** the delay before each retry, in seconds, for a policy with `max_attempts` attempts. */
export function retryDelays(policy: RetryPolicy): number[] {
  const attempts = Math.floor(policy.max_attempts);

  if (!Number.isFinite(attempts) || attempts <= 1) {
    return [];
  }

  const base = Math.max(0, Math.floor(policy.backoff_base_seconds));
  const cap = Math.max(base, Math.floor(policy.backoff_max_seconds));
  const delays: number[] = [];

  for (let attempt = 1; attempt < attempts; attempt += 1) {
    // the runtime clamps the exponent at 30 before shifting; mirror it so a silly base does not
    // preview an Infinity the backend would never produce.
    const exponent = Math.min(attempt - 1, 30);
    delays.push(Math.min(Math.max(base * 2 ** exponent, base), cap));
  }

  return delays;
}

/** `90s` / `2m 30s` / `1h 5m`, whichever reads shortest. */
export function formatDuration(seconds: number): string {
  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;

  if (minutes < 60) {
    return remainder ? `${String(minutes)}m ${String(remainder)}s` : `${String(minutes)}m`;
  }

  const hours = Math.floor(minutes / 60);
  const minutesLeft = minutes % 60;
  return minutesLeft ? `${String(hours)}h ${String(minutesLeft)}m` : `${String(hours)}h`;
}

/** a one-line reading of what the policy will actually do. */
export function describeRetryPolicy(policy: RetryPolicy): string {
  const delays = retryDelays(policy);

  if (delays.length === 0) {
    return "Runs once; a failure goes straight to its transition.";
  }

  const retryWord = delays.length === 1 ? "retry" : "retries";
  const schedule = delays.map(formatDuration).join(", then ");
  const spread = policy.jitter ? ", each randomized down to half" : "";
  const cause =
    RETRY_CLASSES.find((entry) => entry.value === policy.retry_on)?.label.toLowerCase() ??
    "failure or timeout";
  return `Up to ${String(delays.length)} ${retryWord} on ${cause}, waiting ${schedule}${spread}.`;
}

/** the total time the retries can add before the node gives up, ignoring the runs themselves. */
export function retryWindowSeconds(policy: RetryPolicy): number {
  return retryDelays(policy).reduce((total, delay) => total + delay, 0);
}
