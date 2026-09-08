//! best-effort native notifications for desktop connection health, work, and profile approvals.
//! platform delivery runs off the caller's thread and never blocks the worker.

/// post a "went degraded" toast (broker unreachable / worker loop crash-looping).
pub fn notify_degraded(detail: &str) {
    toast(
        "Runinator Desktop Agent reconnecting",
        format!("Reconnecting to the broker. {detail}")
            .trim()
            .to_string(),
    );
}

/// post a "gave up" toast: the reconnect budget is spent and the agent has stopped itself, so this
/// machine is no longer taking work until someone starts it again.
pub fn notify_disconnected(attempts: u32) {
    toast(
        "Runinator Desktop Agent disconnected",
        format!(
            "Stopped after {attempts} failed reconnect attempts. Open the agent and press Start \
             once the service is reachable."
        ),
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
        #[cfg(target_os = "macos")]
        initialize_application();
        if let Err(error) = notify_rust::Notification::new()
            .summary(summary)
            .body(&body)
            .show()
        {
            tracing::warn!(%error, "desktop notification could not be delivered");
        }
    });
}

/// announce work actually executing on this desktop.
pub fn notify_action_started(provider: &str, function: &str) {
    toast(
        "Runinator action running",
        format!("Executing {provider}.{function} on this desktop."),
    );
}

/// request approval of a new or changed collection specification.
pub fn notify_profile_approval(name: &str) {
    toast(
        "Runinator profile approval required",
        format!(
            "Execution profile '{name}' needs local approval. Open Execution profiles in the desktop agent to review it."
        ),
    );
}

/// announce an explicitly requested collection before a source can wait for OS approval.
pub fn notify_profile_collection(name: &str, operation: &str) {
    toast(
        "Runinator profile collection running",
        format!(
            "Execution profile '{name}': {operation}. Check for a system access prompt if collection is waiting."
        ),
    );
}

/// direct an operator to collection failure details retained in the agent.
pub fn notify_profile_failed(name: &str) {
    toast(
        "Runinator profile collection failed",
        format!(
            "Execution profile '{name}' could not be collected. Open Execution profiles in the desktop agent for details."
        ),
    );
}

#[cfg(target_os = "macos")]
fn initialize_application() {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();
    INITIALIZED.call_once(|| {
        // packaged agents should identify their own app instead of the library's Finder fallback.
        let bundled = std::env::current_exe().ok().is_some_and(|path| {
            path.parent()
                .and_then(std::path::Path::parent)
                .is_some_and(|contents| contents.join("Info.plist").is_file())
        });
        if bundled && let Err(error) = notify_rust::set_application("dev.runinator.desktop-agent") {
            tracing::warn!(%error, "desktop notification identity could not be initialized");
        }
    });
}
