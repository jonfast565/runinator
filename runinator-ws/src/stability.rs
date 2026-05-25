use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

static RESULT_EVENTS_APPLIED: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_DUPLICATE: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_RETRIED: AtomicU64 = AtomicU64::new(0);
static RESULT_EVENTS_DEAD_LETTERED: AtomicU64 = AtomicU64::new(0);
static RESULT_RECEIVE_ERRORS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StabilityCounters {
    pub result_events_applied: u64,
    pub result_events_duplicate: u64,
    pub result_events_retried: u64,
    pub result_events_dead_lettered: u64,
    pub result_receive_errors: u64,
}

pub(crate) fn result_event_applied(applied: bool) {
    if applied {
        RESULT_EVENTS_APPLIED.fetch_add(1, Ordering::Relaxed);
    } else {
        RESULT_EVENTS_DUPLICATE.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn result_event_retried() {
    RESULT_EVENTS_RETRIED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn result_event_dead_lettered() {
    RESULT_EVENTS_DEAD_LETTERED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn result_receive_error() {
    RESULT_RECEIVE_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> StabilityCounters {
    StabilityCounters {
        result_events_applied: RESULT_EVENTS_APPLIED.load(Ordering::Relaxed),
        result_events_duplicate: RESULT_EVENTS_DUPLICATE.load(Ordering::Relaxed),
        result_events_retried: RESULT_EVENTS_RETRIED.load(Ordering::Relaxed),
        result_events_dead_lettered: RESULT_EVENTS_DEAD_LETTERED.load(Ordering::Relaxed),
        result_receive_errors: RESULT_RECEIVE_ERRORS.load(Ordering::Relaxed),
    }
}
