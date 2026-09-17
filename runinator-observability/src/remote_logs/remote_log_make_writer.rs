#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct RemoteLogMakeWriter {
    pub(super) source: String,
}

impl<'a> MakeWriter<'a> for RemoteLogMakeWriter {
    type Writer = RemoteLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RemoteLogWriter {
            source: self.source.clone(),
            buffer: Vec::new(),
        }
    }
}
