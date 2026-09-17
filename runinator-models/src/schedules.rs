//! scheduling policy: how many runs of a workflow may overlap, what happens to cron slots missed
//! while the engine was down, and the freeze windows that suspend firing entirely.

use crate::rbac::ScopeRef;
use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, positive_limit,
    required_text,
};

fn default_schedule_timezone() -> String {
    "UTC".to_string()
}

/// Supported recurrence languages. `Weekdays` is the ergonomic form for the most common weekly
/// rule; `Rrule` accepts an RFC 5545 RRULE body and keeps DTSTART separate for safer editing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleRecurrence {
    Once {
        at: DateTime<Utc>,
    },
    Cron {
        expression: String,
    },
    Weekdays {
        days: Vec<ScheduleWeekday>,
        hour: u8,
        minute: u8,
        #[serde(default)]
        second: u8,
    },
    Rrule {
        /// RFC 5545 rule body, for example `FREQ=WEEKLY;BYDAY=TU,WE`.
        rule: String,
        /// The first occurrence and its local wall-clock time after conversion to `timezone`.
        dtstart: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl ScheduleWeekday {
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];
}

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
    /// A recurring or one-shot blackout excluded the scheduled occurrence.
    ScheduleExcluded,
}

impl FiringOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            FiringOutcome::Fired => "fired",
            FiringOutcome::ConcurrencySkipped => "concurrency_skipped",
            FiringOutcome::CatchupSkipped => "catchup_skipped",
            FiringOutcome::ScheduleExcluded => "schedule_excluded",
        }
    }
}

// hand-written so an empty batch does not require the run type to be `Default`.

/// per-request cap on backfilled slots. a year of a minutely cron is half a million runs, so the
/// endpoint refuses to be the thing that fills the run table by accident.
pub const DEFAULT_BACKFILL_LIMIT: i64 = 100;

/// the maximum a caller may raise [`BackfillRequest::limit`] to.
pub const MAX_BACKFILL_LIMIT: i64 = 1000;

#[cfg(test)]
#[path = "schedules_tests.rs"]
mod tests;

mod schedule_spec;
pub use schedule_spec::ScheduleSpec;

mod calendar_subscription;
pub use calendar_subscription::CalendarSubscription;

mod new_calendar_subscription_record;
pub use new_calendar_subscription_record::NewCalendarSubscriptionRecord;

mod calendar_subscription_secret;
pub use calendar_subscription_secret::CalendarSubscriptionSecret;

mod workflow_concurrency;
pub use workflow_concurrency::WorkflowConcurrency;

mod trigger_catchup;
pub use trigger_catchup::TriggerCatchup;

mod freeze_window;
pub use freeze_window::FreezeWindow;

mod new_freeze_window;
pub use new_freeze_window::NewFreezeWindow;

mod trigger_firing_batch;
pub use trigger_firing_batch::TriggerFiringBatch;

mod backfill_request;
pub use backfill_request::BackfillRequest;

mod backfill_response;
pub use backfill_response::BackfillResponse;
