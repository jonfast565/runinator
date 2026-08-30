//! scheduling policy: how many runs of a workflow may overlap, what happens to cron slots missed
//! while the engine was down, and the freeze windows that suspend firing entirely.

use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, positive_limit,
    required_text,
};

/// what the trigger loop does when a workflow is already at its concurrency limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    /// start the run anyway; overlapping runs are the workflow's problem. the historical behavior.
    #[default]
    Allow,
    /// drop the slot: record the firing so it is never retried, and advance to the next one.
    Skip,
    /// leave the slot due and re-evaluate on the next tick, so it fires once capacity frees up.
    /// nothing is created while blocked, so a blocked schedule costs no runs and no wakes.
    Queue,
    /// cancel the workflow's in-flight runs, then start this one.
    CancelPrevious,
}

impl ConcurrencyPolicy {
    /// every policy, default first. the UI catalog reads this so the option list cannot drift from
    /// the variants the trigger loop actually honors.
    pub const ALL: [Self; 4] = [Self::Allow, Self::Skip, Self::Queue, Self::CancelPrevious];

    pub fn as_str(self) -> &'static str {
        match self {
            ConcurrencyPolicy::Allow => "allow",
            ConcurrencyPolicy::Skip => "skip",
            ConcurrencyPolicy::Queue => "queue",
            ConcurrencyPolicy::CancelPrevious => "cancel_previous",
        }
    }

    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw {
            "allow" => Some(ConcurrencyPolicy::Allow),
            "skip" => Some(ConcurrencyPolicy::Skip),
            "queue" => Some(ConcurrencyPolicy::Queue),
            "cancel_previous" => Some(ConcurrencyPolicy::CancelPrevious),
            _ => None,
        }
    }
}

/// a workflow's concurrency limit, read from `definition.metadata.concurrency`. absent metadata
/// means [`WorkflowConcurrency::unlimited`], which is the pre-policy behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowConcurrency {
    /// the number of non-terminal runs allowed at once. `0` means unlimited.
    #[serde(default)]
    pub max_concurrent_runs: i64,
    #[serde(default)]
    pub on_conflict: ConcurrencyPolicy,
}

impl Default for WorkflowConcurrency {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl WorkflowConcurrency {
    pub const fn unlimited() -> Self {
        Self {
            max_concurrent_runs: 0,
            on_conflict: ConcurrencyPolicy::Allow,
        }
    }

    /// true when this policy can ever decline a firing. an unlimited or `allow` policy never does,
    /// so the trigger loop can skip counting active runs entirely.
    pub fn is_enforced(&self) -> bool {
        self.max_concurrent_runs > 0 && self.on_conflict != ConcurrencyPolicy::Allow
    }

    /// read the policy out of a workflow graph's `metadata` object. an unparseable or missing
    /// `concurrency` entry falls back to unlimited rather than failing the firing.
    pub fn from_metadata(metadata: &Value) -> Self {
        metadata
            .get("concurrency")
            .and_then(|value| serde_json::from_value(value.clone().into()).ok())
            .unwrap_or_else(Self::unlimited)
    }
}

/// what happens to cron slots that came due while nothing was firing them (engine downtime, a
/// freeze window, or a `queue` policy holding the schedule back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CatchupPolicy {
    /// collapse the whole backlog into a single run, then re-anchor to the next future slot. the
    /// historical behavior, and the default.
    #[default]
    FireOnce,
    /// replay every missed slot as its own run, up to [`TriggerCatchup::max_slots`].
    FireAll,
    /// abandon slots that came due more than [`TriggerCatchup::grace_seconds`] ago and re-anchor.
    Skip,
}

impl CatchupPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            CatchupPolicy::FireOnce => "fire_once",
            CatchupPolicy::FireAll => "fire_all",
            CatchupPolicy::Skip => "skip",
        }
    }

    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw {
            "fire_once" => Some(CatchupPolicy::FireOnce),
            "fire_all" => Some(CatchupPolicy::FireAll),
            "skip" => Some(CatchupPolicy::Skip),
            _ => None,
        }
    }
}

/// default lateness a `skip` catch-up tolerates before treating a slot as missed. every firing is
/// slightly late (the loop polls), so without a grace `skip` would drop every run.
pub const DEFAULT_CATCHUP_GRACE_SECONDS: i64 = 60;

/// how many slots one `fire_all` catch-up replays per trigger per tick. bounded so a trigger that
/// was down for a week cannot flood the run table and the wake queue in a single pass.
pub const DEFAULT_CATCHUP_MAX_SLOTS: i64 = 25;

/// a trigger's catch-up policy, read from its `configuration.catchup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerCatchup {
    #[serde(default)]
    pub policy: CatchupPolicy,
    /// lateness a `skip` policy tolerates. unused by the other policies.
    #[serde(default)]
    pub grace_seconds: Option<i64>,
    /// per-tick replay cap for `fire_all`. unused by the other policies.
    #[serde(default)]
    pub max_slots: Option<i64>,
}

impl Default for TriggerCatchup {
    fn default() -> Self {
        Self {
            policy: CatchupPolicy::FireOnce,
            grace_seconds: None,
            max_slots: None,
        }
    }
}

impl TriggerCatchup {
    pub fn grace(&self) -> i64 {
        self.grace_seconds
            .filter(|seconds| *seconds > 0)
            .unwrap_or(DEFAULT_CATCHUP_GRACE_SECONDS)
    }

    pub fn max_slots(&self) -> i64 {
        self.max_slots
            .filter(|slots| *slots > 0)
            .unwrap_or(DEFAULT_CATCHUP_MAX_SLOTS)
    }

    /// read the policy out of a trigger's `configuration` object. a `catchup` entry may be either
    /// the bare policy string (`"fire_all"`) or the full object.
    pub fn from_configuration(configuration: &Value) -> Self {
        let Some(catchup) = configuration.get("catchup") else {
            return Self::default();
        };
        if let Some(raw) = catchup.as_str() {
            return Self {
                policy: CatchupPolicy::from_str_opt(raw).unwrap_or_default(),
                ..Self::default()
            };
        }

        serde_json::from_value(catchup.clone().into()).unwrap_or_default()
    }
}

/// a scheduled suspension of trigger firing. a window with no `workflow_id` freezes every workflow
/// in its org; one with no `org_id` freezes the whole platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeWindow {
    pub id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFreezeWindow {
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Validate for NewFreezeWindow {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        optional_text("reason", self.reason.as_deref(), LONG_TEXT_MAX)?;
        if self.ends_at <= self.starts_at {
            return Err(ValidationError::new(
                "ends_at",
                "must be later than starts_at",
            ));
        }
        Ok(())
    }
}

fn default_enabled() -> bool {
    true
}

/// why a due slot produced no run. recorded on the firing row so a missing run is explainable
/// after the fact instead of looking like a lost schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiringOutcome {
    /// a run was created for the slot.
    Fired,
    /// the concurrency policy declined the slot.
    ConcurrencySkipped,
    /// the catch-up policy abandoned the slot as too far past.
    CatchupSkipped,
}

impl FiringOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            FiringOutcome::Fired => "fired",
            FiringOutcome::ConcurrencySkipped => "concurrency_skipped",
            FiringOutcome::CatchupSkipped => "catchup_skipped",
        }
    }
}

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
}

// hand-written so an empty batch does not require the run type to be `Default`.
impl<R> Default for TriggerFiringBatch<R> {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            canceled_run_ids: Vec::new(),
            concurrency_skipped: 0,
            concurrency_deferred: 0,
            catchup_skipped: 0,
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
        self.concurrency_skipped > 0 || self.concurrency_deferred > 0 || self.catchup_skipped > 0
    }
}

/// the time range a manual backfill replays. inclusive of `to`, exclusive of `from`, matching the
/// cron iterator's own half-open stepping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillRequest {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// cap on the number of slots replayed. defaults to [`DEFAULT_BACKFILL_LIMIT`].
    #[serde(default)]
    pub limit: Option<i64>,
    /// when true, report the slots that would fire without creating any runs.
    #[serde(default)]
    pub dry_run: bool,
}

impl Validate for BackfillRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.to <= self.from {
            return Err(ValidationError::new("to", "must be later than from"));
        }
        positive_limit("limit", self.limit, MAX_BACKFILL_LIMIT)
    }
}

/// per-request cap on backfilled slots. a year of a minutely cron is half a million runs, so the
/// endpoint refuses to be the thing that fills the run table by accident.
pub const DEFAULT_BACKFILL_LIMIT: i64 = 100;

/// the maximum a caller may raise [`BackfillRequest::limit`] to.
pub const MAX_BACKFILL_LIMIT: i64 = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResponse {
    pub trigger_id: Uuid,
    pub workflow_id: Uuid,
    /// slots inside the range that already had a firing recorded, so they were left alone.
    pub already_fired: i64,
    /// slots that produced a run, or would have on a dry run.
    pub fired: i64,
    /// true when the range held more slots than `limit` allowed.
    pub truncated: bool,
    pub dry_run: bool,
    pub run_ids: Vec<Uuid>,
    pub slots: Vec<DateTime<Utc>>,
}

#[cfg(test)]
#[path = "schedules_tests.rs"]
mod tests;
