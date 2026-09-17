#[allow(unused_imports)]
use super::*;

pub(super) struct ReadBudget {
    pub(super) remaining: u64,
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self {
            remaining: MAX_PACK_UNCOMPRESSED_BYTES,
        }
    }
}

impl ReadBudget {
    pub(super) fn read(
        &mut self,
        reader: &mut impl Read,
        name: &str,
    ) -> Result<Vec<u8>, PackError> {
        let limit = self.remaining.min(MAX_PACK_ENTRY_BYTES);
        let mut bytes = Vec::new();
        reader.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(format!(
                "pack entry '{name}' exceeds its uncompressed read budget of {limit} bytes"
            )
            .into());
        }
        self.remaining -= bytes.len() as u64;
        Ok(bytes)
    }
}
