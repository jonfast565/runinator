#[allow(unused_imports)]
use super::*;

pub(super) struct DiskEntries {
    pub(super) reader: Option<BufReader<File>>,
    pub(super) packs: Arc<Vec<Id>>,
    pub(super) remaining: u64,
}

impl DiskEntries {
    pub(super) fn next(&mut self) -> Result<Option<Location>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let mut bytes = [0; ENTRY_BYTES as usize];
        self.reader
            .as_mut()
            .ok_or_else(|| corrupt("missing index reader"))?
            .read_exact(&mut bytes)?;
        self.remaining -= 1;
        decode_compact(bytes, &self.packs).map(Some)
    }
}
