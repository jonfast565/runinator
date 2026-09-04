import type { ReplayPlan } from "../domain/models/workflow/replay";

type Reviewer = (plan: ReplayPlan) => Promise<boolean>;
let reviewer: Reviewer | undefined;
let pending: Promise<unknown> = Promise.resolve();

export function setReplayPlanReviewer(value: Reviewer | undefined) {
  reviewer = value;
}

// bulk and concurrent entrypoints share one review queue; absent UI never implies consent.
export function reviewReplayPlan(plan: ReplayPlan): Promise<boolean> {
  const result = pending.then(() => reviewer?.(plan) ?? false);
  pending = result.catch(() => false);
  return result;
}
