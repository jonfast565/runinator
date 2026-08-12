//! covers the sticky stop latch.

use super::*;

// the reason this is not a bare `Notify`: `notify_waiters` only wakes waiters that already exist, so
// a host that starts an agent and stops it before the lifecycle task has parked would otherwise
// leave the agent running with nobody left to tell it.
#[tokio::test]
async fn a_stop_before_anyone_waits_is_still_observed() {
    let shutdown = Shutdown::new();
    shutdown.trigger();
    assert!(shutdown.is_stopping());
    assert!(shutdown.sleep_or_stop(Duration::from_secs(30)).await);
}

#[tokio::test]
async fn a_stop_during_a_backoff_returns_early() {
    let shutdown = Shutdown::new();
    let waiter = shutdown.clone();
    let task = tokio::spawn(async move { waiter.sleep_or_stop(Duration::from_secs(30)).await });

    // let the waiter park before triggering, so this exercises the notify path rather than the latch.
    tokio::time::sleep(Duration::from_millis(50)).await;
    shutdown.trigger();

    let stopped = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the backoff should end as soon as shutdown fires")
        .expect("the waiter should not panic");
    assert!(stopped);
}

#[tokio::test]
async fn an_uninterrupted_delay_reports_no_stop() {
    let shutdown = Shutdown::new();
    assert!(!shutdown.sleep_or_stop(Duration::from_millis(10)).await);
    assert!(!shutdown.is_stopping());
}
