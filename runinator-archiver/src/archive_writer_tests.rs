//! archive finalization must propagate buffered write and flush failures.

use super::*;

struct FailingWriter {
    fail_write: bool,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            Err(io::Error::other("final buffered write failed"))
        } else {
            Ok(bytes.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("final flush failed"))
    }
}

#[test]
fn finalization_propagates_buffered_io_failures() {
    for fail_write in [false, true] {
        let writer = BufWriter::new(FailingWriter { fail_write });
        let mut encoder = GzEncoder::new(writer, Compression::default());
        encoder.write_all(b"{\"id\":1}\n").unwrap();
        let result = finish_archive(encoder);
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Other));
    }
}
