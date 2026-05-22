use super::*;
use runinator_models::runs::ProviderExecutionEvent;
use std::sync::Mutex;

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
