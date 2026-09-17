#[allow(unused_imports)]
use super::*;

pub struct ObjectReader {
    pub meta: ObjectMeta,
    /// present when the read was ranged; `None` means the reader covers the whole object.
    pub range: Option<ResolvedRange>,
    pub body: Box<dyn AsyncRead + Send + Unpin>,
}

impl ObjectReader {
    /// how many bytes this reader will yield.
    pub fn len(&self) -> u64 {
        self.range
            .map(|range| range.length)
            .unwrap_or(self.meta.size)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
