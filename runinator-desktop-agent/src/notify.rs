//! best-effort native desktop notifications for the agent's connection health, so an operator not
//! watching the window (or the menu-bar icon) still hears when this machine drops off the broker and
//! when it comes back. purely advisory: a platform that can't post a toast just gets nothing, and the
//! call never blocks the caller — the actual `show()` runs on a detached thread.

/// post a "went degraded" toast (broker unreachable / worker loop crash-looping).
pub fn notify_degraded(detail: &str) {
    toast(
        "Runinator Desktop Agent disconnected",
        format!("Reconnecting to the broker. {detail}")
            .trim()
            .to_string(),
    );
}

/// post a "recovered" toast once the worker loop is back up after a degraded episode.
pub fn notify_recovered() {
    toast(
        "Runinator Desktop Agent reconnected",
        "The worker loop is running again.".to_string(),
    );
}

// fire-and-forget: notify-rust's `show()` can touch platform IPC, so run it off the caller's thread
// and swallow any error — a missing notification must never affect the agent's runtime.
fn toast(summary: &'static str, body: String) {
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .summary(summary)
            .body(&body)
            .show();
    });
}
