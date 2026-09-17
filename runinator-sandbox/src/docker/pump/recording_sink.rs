#[allow(unused_imports)]
use super::*;

/// a [`LineSink`] that records what it saw, for tests and for callers that want the lines twice.
#[derive(Default)]
pub struct RecordingSink {
    pub(super) lines: Mutex<Vec<(Stream, String)>>,
}

impl RecordingSink {
    pub fn lines(&self) -> Vec<(Stream, String)> {
        self.lines
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default()
    }
}

impl LineSink for RecordingSink {
    fn line(&self, stream: Stream, text: &str) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.push((stream, text.to_string()));
        }
    }
}
