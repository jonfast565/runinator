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

mod log_make_writer;
pub use log_make_writer::LogMakeWriter;

mod log_writer;
pub use log_writer::LogWriter;
