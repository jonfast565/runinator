/** `state.compensation` saga-rollback bookkeeping. */
export interface CompensationFrame {
  remaining?: string[];
  active_run_id?: string | null;
}
