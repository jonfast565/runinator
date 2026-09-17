#[allow(unused_imports)]
use super::*;

pub(super) struct FileWriter {
    pub(super) file: Arc<Mutex<File>>,
}

impl Write for FileWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        // a poisoned lock still yields the guard; a dropped file write is not worth panicking over.
        let mut file = match self.file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };
        file.write_all(data)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.lock() {
            Ok(mut file) => file.flush(),
            Err(poisoned) => poisoned.into_inner().flush(),
        }
    }
}
