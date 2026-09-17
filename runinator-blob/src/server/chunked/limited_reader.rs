#[allow(unused_imports)]
use super::*;

pub struct LimitedReader<R> {
    pub(super) inner: R,
    pub(super) limit: u64,
    pub(super) consumed: u64,
}

impl<R> LimitedReader<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            consumed: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LimitedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        target: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if target.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let remaining = self.limit.saturating_sub(self.consumed);
        let allowed = usize::try_from(remaining.saturating_add(1))
            .unwrap_or(usize::MAX)
            .min(target.remaining());
        let unfilled = target.initialize_unfilled_to(allowed);
        let mut limited = ReadBuf::new(unfilled);
        match Pin::new(&mut self.inner).poll_read(cx, &mut limited) {
            Poll::Ready(Ok(())) => {
                let read = limited.filled().len();
                let consumed = self.consumed.saturating_add(read as u64);
                if consumed > self.limit {
                    return Poll::Ready(Err(invalid(
                        "decoded upload exceeds the configured limit",
                    )));
                }
                target.advance(read);
                self.consumed = consumed;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}
