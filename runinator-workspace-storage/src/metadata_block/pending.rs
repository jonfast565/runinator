#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub struct Pending {
    pub(super) entries: Vec<(Id, Kind, Vec<u8>)>,
    pub(super) payload_bytes: usize,
}

impl Pending {
    pub fn would_overflow(&self, raw_len: usize) -> bool {
        HEADER
            .saturating_add((self.entries.len() + 1).saturating_mul(MEMBER_HEADER))
            .saturating_add(self.payload_bytes)
            .saturating_add(raw_len)
            > BLOCK_LIMIT
            || self.entries.len() >= MAX_MEMBERS
    }

    pub fn push(&mut self, id: Id, kind: Kind, raw: &[u8]) -> Result<()> {
        if self.would_overflow(raw.len()) || !eligible(kind, raw) {
            return Err(invalid("invalid metadata-block insertion"));
        }
        if Id::object(kind, raw) != id {
            return Err(corrupt("metadata member identity mismatch"));
        }
        self.payload_bytes = self
            .payload_bytes
            .checked_add(raw.len())
            .ok_or_else(|| invalid("metadata block size overflow"))?;
        self.entries.push((id, kind, raw.to_vec()));
        Ok(())
    }

    pub fn take(&mut self) -> Result<Option<(Vec<u8>, Vec<Id>)>> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        self.entries.sort_unstable_by_key(|entry| entry.0);
        if self
            .entries
            .windows(2)
            .any(|window| window[0].0 == window[1].0)
        {
            return Err(corrupt("duplicate metadata block member"));
        }
        let mut output =
            Vec::with_capacity(HEADER + self.entries.len() * MEMBER_HEADER + self.payload_bytes);
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        let mut ids = Vec::with_capacity(self.entries.len());
        let mut offset = HEADER + self.entries.len() * MEMBER_HEADER;
        for (id, kind, raw) in &self.entries {
            output.extend_from_slice(&id.0);
            output.push(*kind as u8);
            output.extend_from_slice(&[0; 3]);
            output.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += raw.len();
            ids.push(*id);
        }
        for (_, _, raw) in self.entries.drain(..) {
            output.extend_from_slice(&raw);
        }
        self.payload_bytes = 0;
        Ok(Some((output, ids)))
    }
}
