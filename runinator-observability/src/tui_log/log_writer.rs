#[allow(unused_imports)]
use super::*;

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
