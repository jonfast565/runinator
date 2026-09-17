#[allow(unused_imports)]
use super::*;

/// the result of one trigger-loop claim pass. runs were created; `canceled_run_ids` were set
/// terminal by a `cancel_previous` policy and still need their workers told; the counters are
/// observability for slots that deliberately produced nothing.
#[derive(Debug, Clone)]
pub struct TriggerFiringBatch<R> {
    pub runs: Vec<R>,
    pub canceled_run_ids: Vec<Uuid>,
    pub concurrency_skipped: u64,
    pub concurrency_deferred: u64,
    pub catchup_skipped: u64,
    pub schedule_excluded: u64,
}

impl<R> Default for TriggerFiringBatch<R> {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            canceled_run_ids: Vec::new(),
            concurrency_skipped: 0,
            concurrency_deferred: 0,
            catchup_skipped: 0,
            schedule_excluded: 0,
        }
    }
}

impl<R> TriggerFiringBatch<R> {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// true when the pass declined at least one slot, so the caller knows there is something worth
    /// logging even though no runs were created.
    pub fn declined_any(&self) -> bool {
        self.concurrency_skipped > 0
            || self.concurrency_deferred > 0
            || self.catchup_skipped > 0
            || self.schedule_excluded > 0
    }
}
