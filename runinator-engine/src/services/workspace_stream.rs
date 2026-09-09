//! Bounded bridge from synchronous paged reads to an HTTP response body.
use std::io::Write;

pub struct Writer {
    runtime: tokio::runtime::Handle,
    stream: tokio::io::DuplexStream,
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        self.runtime.block_on(self.stream.write(bytes))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.runtime.block_on(self.stream.flush())
    }
}

pub fn stream<F>(size: u64, produce: F) -> crate::artifact_storage::ArtifactContent
where
    F: FnOnce(Writer) -> Result<(), runinator_models::errors::SendableError> + Send + 'static,
{
    let (reader, writer) = tokio::io::duplex(512 * 1024);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = produce(Writer {
            runtime,
            stream: writer,
        }) {
            tracing::warn!(%error, "workspace stream stopped before completion");
        }
    });
    crate::artifact_storage::ArtifactContent {
        size_bytes: size,
        body: Box::new(reader),
    }
}
