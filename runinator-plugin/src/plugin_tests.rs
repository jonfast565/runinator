use super::*;
use crate::cancel::CancellationToken;
use runinator_models::runs::ProviderExecutionEvent;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[derive(Default)]
struct MemorySink {
    events: Mutex<Vec<ProviderExecutionEvent>>,
}

impl ProviderEventSink for MemorySink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[test]
fn polls_valid_events_and_skips_malformed_lines() {
    let events_path = unique_temp_file("events-test", "jsonl");
    fs::create_dir_all(events_path.parent().unwrap()).unwrap();
    fs::write(
        &events_path,
        concat!(
            "{\"type\":\"chunk\",\"stream\":\"stdout\",\"content\":\"hello\"}\n",
            "not json\n",
            "{\"type\":\"message\",\"message\":\"done\"}\n",
        ),
    )
    .unwrap();

    let sink = MemorySink::default();
    let offset = poll_events_once(&events_path, 0, &sink).unwrap();
    assert!(offset > 0);

    let events = sink.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        ProviderExecutionEvent::Chunk { stream, content }
            if stream == "stdout" && content == "hello"
    ));
    assert!(matches!(
        &events[1],
        ProviderExecutionEvent::Message { message } if message == "done"
    ));
}

#[test]
fn touches_signal_file_when_token_is_cancelled() {
    let signal_path = unique_temp_file("cancel-test", "signal");
    fs::create_dir_all(signal_path.parent().unwrap()).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let stop = Arc::new(AtomicBool::new(false));
    signal_cancel_until_stopped(signal_path.clone(), token, stop);
    assert!(signal_path.exists());
    let _ = fs::remove_file(&signal_path);
}

#[test]
fn leaves_signal_file_absent_without_cancellation() {
    let signal_path = unique_temp_file("cancel-test", "signal");
    fs::create_dir_all(signal_path.parent().unwrap()).unwrap();
    let token = CancellationToken::new();
    let stop = Arc::new(AtomicBool::new(true));
    signal_cancel_until_stopped(signal_path.clone(), token, stop);
    assert!(!signal_path.exists());
}

#[test]
fn cancel_signal_path_is_sibling_of_events() {
    use runinator_models::runs::ProviderExecutionRequest;
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "p".into(),
        action_function: "f".into(),
        parameters: Default::default(),
        timeout_secs: 1,
        artifact_dir: "/tmp/run/artifacts".into(),
        events_jsonl_path: "/tmp/run/events.jsonl".into(),
    };
    assert_eq!(
        request.cancel_signal_path(),
        Some(std::path::PathBuf::from("/tmp/run/cancel.signal"))
    );
    let empty = ProviderExecutionRequest {
        events_jsonl_path: String::new(),
        ..request
    };
    assert_eq!(empty.cancel_signal_path(), None);
}
