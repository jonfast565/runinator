#[allow(unused_imports)]
use super::*;

/// Buffer one tracing event before atomically adding its non-empty physical lines to the rolling
/// pane. A fmt layer constructs a fresh writer per event, so no mutex is needed here.
#[derive(Default)]
pub struct LogWriter {
    pub(super) buffer: Vec<u8>,
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
        let text = String::from_utf8_lossy(&self.buffer);
        for line in text.lines() {
            let line = line.trim_end();
            if !line.is_empty() {
                log_line(line.to_string());
            }
        }
    }
}
