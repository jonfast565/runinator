#[allow(unused_imports)]
use super::*;

pub(super) struct FileMakeWriter {
    pub(super) file: Arc<Mutex<File>>,
}

impl<'a> MakeWriter<'a> for FileMakeWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriter {
            file: self.file.clone(),
        }
    }
}
