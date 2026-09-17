#[allow(unused_imports)]
use super::*;

pub(super) struct Sha256Writer<W> {
    pub(super) inner: W,
    pub(super) hasher: Sha256,
}

impl<W> Sha256Writer<W> {
    pub(super) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    pub(super) fn digest(self) -> Id {
        Id(self.hasher.finalize().into())
    }
}

impl<W: Write> Write for Sha256Writer<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
