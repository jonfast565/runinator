#[allow(unused_imports)]
use super::*;

pub(super) struct ConsoleMakeWriter {
    pub(super) shared: SharedHandle,
}

impl<'a> MakeWriter<'a> for ConsoleMakeWriter {
    type Writer = ConsoleWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ConsoleWriter {
            shared: self.shared.clone(),
            buf: Vec::new(),
        }
    }
}
