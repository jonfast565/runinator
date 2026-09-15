//! Presentation-independent bridge from tracing into an optional terminal log sink.

use std::{
    io::{self, Write},
    sync::{Arc, OnceLock},
};

use tracing_subscriber::fmt::MakeWriter;

type Sink = Arc<dyn Fn(String) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// Install the process terminal log sink before logging is initialized.
pub fn install_sink(sink: impl Fn(String) + Send + Sync + 'static) {
    let _ = SINK.set(Arc::new(sink));
}

/// Whether a terminal log sink is active.
pub fn is_active() -> bool {
    SINK.get().is_some()
}

#[derive(Clone, Copy, Default)]
pub struct LogMakeWriter;

impl<'a> MakeWriter<'a> for LogMakeWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter::default()
    }
}

#[derive(Default)]
pub struct LogWriter {
    buffer: Vec<u8>,
}

impl Write for LogWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        let Some(sink) = SINK.get() else {
            return;
        };
        let text = String::from_utf8_lossy(&self.buffer);
        for line in text
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
        {
            sink(line.to_string());
        }
    }
}
