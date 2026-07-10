use std::time::Duration;

use super::bucket_to_interval;

// two timestamps in the same 300s window must floor to the identical key, so N-up samplers that read
// slightly different wall clocks still converge to one (org, backend, kind, sampled_at) row.
#[test]
fn timestamps_in_the_same_window_bucket_to_one_key() {
    let interval = Duration::from_secs(300);
    // a window-aligned base (1_700_000_100 is a multiple of 300) so both offsets stay in one window.
    let base = chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap();
    let a = base + chrono::Duration::seconds(7);
    let b = base + chrono::Duration::seconds(291);
    assert_eq!(
        bucket_to_interval(a, interval),
        bucket_to_interval(b, interval)
    );
    // the bucketed value is the window start and is itself aligned to the interval.
    assert_eq!(bucket_to_interval(a, interval).timestamp() % 300, 0);
}

// adjacent windows must produce distinct keys so successive samples are not collapsed.
#[test]
fn adjacent_windows_bucket_to_distinct_keys() {
    let interval = Duration::from_secs(300);
    let start = chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap();
    let next = start + chrono::Duration::seconds(300);
    assert_ne!(
        bucket_to_interval(start, interval),
        bucket_to_interval(next, interval)
    );
}

// a zero interval is a degenerate guard: it must not divide-by-zero, just pass the time through.
#[test]
fn zero_interval_passes_through() {
    let now = chrono::DateTime::from_timestamp(1_700_000_123, 0).unwrap();
    assert_eq!(bucket_to_interval(now, Duration::from_secs(0)), now);
}
