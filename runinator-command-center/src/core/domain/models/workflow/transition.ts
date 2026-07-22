// one edge walked by a workflow run, derived from the node-run chain (`prev_node_run_id`).
// `from_node` is null for the run's first node.
export interface NodeTransition {
  from_node: string | null;
  to_node: string;
  reason: string | null;
  node_run_id: string;
  at: string;
}

// an aggregated `from_node -> to_node` edge across all runs of a workflow, with how often
// it was taken and when it was last taken.
export interface NodeTransitionStat {
  from_node: string;
  to_node: string;
  count: number;
  last_reason: string | null;
  last_at: string;
}
