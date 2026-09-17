#[allow(unused_imports)]
use super::*;

/// Writer for a `tracing_subscriber::fmt` layer that forwards each formatted event to the dashboard
/// log pane. The caller still controls event filtering and any persistent log sink.
#[derive(Clone, Copy, Default)]
pub struct LogMakeWriter;

impl<'a> MakeWriter<'a> for LogMakeWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter::default()
    }
}
