//! covers concurrent stream draining, retained output, and provider chunk emission.

use super::*;

use std::sync::Mutex;

#[derive(Default)]
struct RecordingSink(Mutex<Vec<ProviderExecutionEvent>>);

impl ProviderEventSink for RecordingSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn drains_and_emits_every_line_with_its_stream() {
    let sink = Arc::new(RecordingSink::default());
    let stdout = drain_stream(
        std::io::Cursor::new(b"one\ntwo\n"),
        "stdout",
        Some(&(sink.clone() as Arc<dyn ProviderEventSink>)),
        true,
    );
    let stderr = drain_stream(
        std::io::Cursor::new(b"warning\r\n"),
        "stderr",
        Some(&(sink.clone() as Arc<dyn ProviderEventSink>)),
        true,
    );

    assert_eq!(stdout, "one\ntwo\n");
    assert_eq!(stderr, "warning\n");
    let events = sink.0.lock().unwrap();
    assert!(matches!(
        &events[0],
        ProviderExecutionEvent::Chunk { stream, content }
            if stream == "stdout" && content == "one"
    ));
    assert!(matches!(
        &events[2],
        ProviderExecutionEvent::Chunk { stream, content }
            if stream == "stderr" && content == "warning"
    ));
}
