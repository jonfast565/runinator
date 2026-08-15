//! draining a child's stdout and stderr without letting either block or grow without bound.

use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::runner::{LineSink, Stream};

/// what one drained stream produced.
#[derive(Debug, Default)]
pub struct Drained {
    pub text: String,
    pub truncated: bool,
}

/// spawn a thread that reads `reader` to end-of-stream, keeping at most `max_bytes` and handing
/// every line to `sink` as it arrives.
///
/// the thread is what makes this correct rather than an optimisation: a child writing more than a
/// pipe buffer blocks until someone reads, and a parent that only reads *after* the child exits
/// therefore waits forever on a child that is waiting on it.
pub fn spawn<R>(
    reader: R,
    stream: Stream,
    max_bytes: usize,
    sink: Option<Arc<dyn LineSink>>,
) -> JoinHandle<Drained>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut collected = String::new();
        let mut truncated = false;
        let mut lines = BufReader::new(reader).split(b'\n');
        while let Some(Ok(raw)) = lines.next() {
            let line = String::from_utf8_lossy(&raw);
            let line = line.trim_end_matches('\r');
            if let Some(sink) = &sink {
                // the sink sees every line even once the buffer is full: streaming a log and
                // retaining one are different budgets, and dropping live output would hide the tail
                // of exactly the run someone is watching.
                sink.line(stream, line);
            }
            if collected.len() + line.len() + 1 > max_bytes {
                truncated = true;
                continue;
            }
            collected.push_str(line);
            collected.push('\n');
        }
        Drained {
            text: collected,
            truncated,
        }
    })
}

/// a [`LineSink`] that records what it saw, for tests and for callers that want the lines twice.
#[derive(Default)]
pub struct RecordingSink {
    lines: Mutex<Vec<(Stream, String)>>,
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
