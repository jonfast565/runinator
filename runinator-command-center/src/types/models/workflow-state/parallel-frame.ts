/** `state.parallel` fan-out bookkeeping. */
export interface ParallelFrame {
  node_id: string;
  remaining?: string[];
}
