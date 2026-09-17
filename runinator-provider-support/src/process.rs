//! concurrent child-process output capture and provider-event streaming.

use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use runinator_models::runs::ProviderExecutionEvent;
use runinator_plugin::provider::ProviderEventSink;

/// Concurrent drains for one child's piped stdout and stderr.

fn join_stream(handle: Option<JoinHandle<String>>) -> String {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn spawn_stream<R>(
    reader: R,
    stream: &'static str,
    sink: Option<Arc<dyn ProviderEventSink>>,
    retain: bool,
) -> JoinHandle<String>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || drain_stream(reader, stream, sink.as_ref(), retain))
}

fn drain_stream<R>(
    reader: R,
    stream: &'static str,
    sink: Option<&Arc<dyn ProviderEventSink>>,
    retain: bool,
) -> String
where
    R: Read,
{
    let mut retained = String::new();
    for line in BufReader::new(reader).split(b'\n') {
        let raw = match line {
            Ok(raw) => raw,
            Err(error) => {
                emit(sink, "stderr", format!("failed to read {stream}: {error}"));
                break;
            }
        };
        let line = String::from_utf8_lossy(&raw);
        let line = line.trim_end_matches('\r');
        emit(sink, stream, line.to_string());
        if retain {
            retained.push_str(line);
            retained.push('\n');
        }
    }
    retained
}

fn emit(sink: Option<&Arc<dyn ProviderEventSink>>, stream: &str, content: String) {
    let Some(sink) = sink else {
        return;
    };

    sink.emit(ProviderExecutionEvent::Chunk {
        stream: stream.to_string(),
        content,
    });
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;

mod process_output;
pub use process_output::ProcessOutput;

mod process_output_pump;
pub use process_output_pump::ProcessOutputPump;
