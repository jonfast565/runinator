#[allow(unused_imports)]
use super::*;

pub(super) struct Reader<R> {
    pub(super) inner: R,
    pub(super) alive: Arc<AtomicBool>,
    pub(super) bytes: Arc<AtomicU64>,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Reader<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.alive.load(Ordering::Acquire) {
            return std::task::Poll::Ready(Err(std::io::Error::other(
                "transfer cancelled or lease lost",
            )));
        }
        let before = buf.filled().len();
        let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        self.bytes
            .fetch_add((buf.filled().len() - before) as u64, Ordering::Release);
        result
    }
}
