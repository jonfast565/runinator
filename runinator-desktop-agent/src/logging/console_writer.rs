#[allow(unused_imports)]
use super::*;

pub(super) struct ConsoleWriter {
    pub(super) shared: SharedHandle,
    pub(super) buf: Vec<u8>,
}

impl Write for ConsoleWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for ConsoleWriter {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.buf);
        for line in text.lines() {
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                try_log_line(&self.shared, trimmed.to_string());
            }
        }
    }
}
