#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Default)]
pub struct LogMakeWriter;

impl<'a> MakeWriter<'a> for LogMakeWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter::default()
    }
}
