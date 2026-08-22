// the two primitives every graph walk needs: how a node points at another node, and the transition
// keys that carry one.
//
// a leaf module on purpose. `interrupt-regions` and `header-validation` walk the graph and are in
// turn read by `index`, so keeping these here is what stops that from being an import cycle.

import { isJsonRecord } from "../domain/json";
import type { WorkflowDirectTransitionKey } from "../domain/models";

/** Universal status transitions in the order shown by the UI. */
export const directTransitionKeys: WorkflowDirectTransitionKey[] = [
  "next",
  "on_success",
  "on_failure",
  "on_timeout",
  "on_reject",
];

/** the node id inside a `{ "$node": "..." }` reference, or null when the value is not one. */
export function nodeRefId(value: unknown): string | null {
  return isJsonRecord(value) && typeof value.$node === "string" && value.$node.length > 0
    ? value.$node
    : null;
}
