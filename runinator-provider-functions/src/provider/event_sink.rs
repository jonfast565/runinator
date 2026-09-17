#[allow(unused_imports)]
use super::*;

pub(super) struct EventSink(pub(super) Arc<dyn ProviderEventSink>);

impl LineSink for EventSink {
    fn line(&self, stream: Stream, text: &str) {
        self.0.emit(ProviderExecutionEvent::Chunk {
            stream: stream.as_str().to_string(),
            content: text.to_string(),
        });
    }
}
