//! start-latch recovery: a failed or settled lifecycle must not block a later Start.

use super::*;

#[test]
fn start_is_allowed_when_idle() {
    let shared = Shared::default();
    assert!(!should_skip_start(&shared));
}

#[test]
fn start_is_skipped_while_busy() {
    let shared = Shared {
        busy: true,
        ..Shared::default()
    };
    assert!(should_skip_start(&shared));
}

#[test]
fn start_is_allowed_after_a_failed_start() {
    // the GUI shows Start when `running` is false (failed registration, settled stop). a leftover
    // handle must not make that click a no-op — there is no live handle in this fixture, which is
    // the same skip outcome as a finished one (`is_finished` => not live).
    let mut shared = Shared::default();
    shared.status.running = false;
    shared.connection = ConnectionState::Stopped;
    assert!(!should_skip_start(&shared));
}

#[test]
fn start_is_allowed_when_running_flag_is_set_without_a_live_handle() {
    // inconsistent leftover from a lifecycle that settled without being reaped.
    let mut shared = Shared::default();
    shared.status.running = true;
    assert!(!should_skip_start(&shared));
}

// the phase a Start click opens: `busy` alone cannot say whether a start or a stop is in flight, and
// only a start is cancellable.
#[test]
fn a_start_in_flight_offers_cancel() {
    let shared = Shared {
        busy: true,
        starting: true,
        ..Shared::default()
    };
    assert_eq!(control_state(&shared), Control::Starting);
    // and a second Start click during it stays a no-op.
    assert!(should_skip_start(&shared));
}

#[test]
fn a_stop_in_flight_offers_nothing() {
    let shared = Shared {
        busy: true,
        ..Shared::default()
    };
    assert_eq!(control_state(&shared), Control::Stopping);
}

#[test]
fn an_idle_agent_offers_start() {
    assert_eq!(control_state(&Shared::default()), Control::Startable);
}

#[test]
fn desktop_identity_labels_cannot_be_overridden() {
    let config = AgentConfig {
        extra_labels: vec!["runner=other".to_string(), "pool=other".to_string()],
        ..AgentConfig::default()
    };

    let labels = advertised_labels(&config);

    assert_eq!(labels.get("runner").map(String::as_str), Some(POOL_LABEL));
    assert_eq!(labels.get("pool").map(String::as_str), Some(POOL_LABEL));
}

// a lifecycle that settled (no live handle) is startable again whatever the `running` flag says,
// which is the same leftover case `start_is_allowed_when_running_flag_is_set_without_a_live_handle`
// covers for the start latch.
#[test]
fn a_settled_lifecycle_is_startable_not_running() {
    let mut shared = Shared::default();
    shared.status.running = true;
    assert_eq!(control_state(&shared), Control::Startable);
}

#[test]
fn resource_history_keeps_the_same_one_minute_window_as_the_tui() {
    let mut history = ResourceHistory::default();
    for index in 0..=RESOURCE_HISTORY_CAPACITY {
        history.push(ResourceSample {
            host_cpu_percent: index as f32,
            ..ResourceSample::default()
        });
    }

    let samples = history.samples().collect::<Vec<_>>();
    assert_eq!(samples.len(), RESOURCE_HISTORY_CAPACITY);
    assert_eq!(samples[0].host_cpu_percent, 1.0);
    assert_eq!(
        samples.last().map(|sample| sample.host_cpu_percent),
        Some(RESOURCE_HISTORY_CAPACITY as f32)
    );
}

#[test]
fn activity_age_only_resets_when_the_work_changes() {
    let mut activity = Activity::default();
    set_activity(&mut activity, "waiting for desktop work");
    let since = activity.since;
    set_activity(&mut activity, "waiting for desktop work");

    assert_eq!(activity.since, since);
    set_activity(&mut activity, "executing std.echo");
    assert_eq!(activity.label, "executing std.echo");
    assert!(activity.since >= since);
}

#[test]
fn desktop_console_describes_worker_output_chunks() {
    let line = describe_worker_event(&WorkerEvent::EffectOutputChunk {
        workflow_run_id: Uuid::nil(),
        effect_id: Uuid::nil(),
        stream: "stderr".into(),
        content: "warning".into(),
    });

    assert_eq!(line, "[stderr · run 00000000] warning");
}
