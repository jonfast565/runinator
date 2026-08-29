//! concurrent child-process output capture and provider-event streaming.

use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use runinator_models::runs::ProviderExecutionEvent;
use runinator_plugin::provider::ProviderEventSink;

/// Output retained while both streams were also emitted to the provider event sink.
#[derive(Debug, Default)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Concurrent drains for one child's piped stdout and stderr.
pub struct ProcessOutputPump {
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
}

impl ProcessOutputPump {
    /// Take both piped streams from `child` and begin draining them immediately.
    pub fn start(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> std::io::Result<Self> {
        Self::start_with_retention(child, sink, true)
    }

    /// Begin draining and streaming without retaining a second in-memory copy.
    pub fn start_discarding(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> std::io::Result<Self> {
        Self::start_with_retention(child, sink, false)
    }

    fn start_with_retention(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
        retain: bool,
    ) -> std::io::Result<Self> {
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::other("child stdout is unavailable; configure it as piped")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            std::io::Error::other("child stderr is unavailable; configure it as piped")
        })?;
        Ok(Self {
            stdout: Some(spawn_stream(stdout, "stdout", sink.clone(), retain)),
            stderr: Some(spawn_stream(stderr, "stderr", sink, retain)),
        })
    }

    /// Wait for both streams to reach EOF and return the retained text.
    pub fn finish(mut self) -> ProcessOutput {
        ProcessOutput {
            stdout: join_stream(self.stdout.take()),
            stderr: join_stream(self.stderr.take()),
        }
    }
}

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
    if let Some(sink) = sink {
        sink.emit(ProviderExecutionEvent::Chunk {
            stream: stream.to_string(),
            content,
        });
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
