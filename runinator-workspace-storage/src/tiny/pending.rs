#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub struct Pending {
    pub(super) entries: Vec<(Id, Vec<u8>)>,
    pub(super) payload_bytes: usize,
}

impl Pending {
    pub fn would_overflow(&self, raw_len: usize) -> bool {
        if raw_len > BLOCK_LIMIT {
            return true;
        }
        HEADER + (self.entries.len() + 1) * MEMBER_HEADER + self.payload_bytes + raw_len
            > BLOCK_LIMIT
            || self.entries.len() >= MAX_MEMBERS
    }
    pub fn push(&mut self, id: Id, raw: &[u8]) -> Result<()> {
        if self.would_overflow(raw.len()) || !eligible(Kind::File, raw)? {
            return Err(invalid("invalid tiny-block insertion"));
        }
        if Id::object(Kind::File, raw) != id {
            return Err(corrupt("tiny member identity mismatch"));
        }
        self.payload_bytes += raw.len();
        self.entries.push((id, raw.to_vec()));
        Ok(())
    }
    /// Returns canonical block bytes and ordered member IDs for index emission.
    pub fn take(&mut self) -> Result<Option<(Vec<u8>, Vec<Id>)>> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        self.entries.sort_unstable_by_key(|entry| entry.0);
        if self.entries.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(corrupt("duplicate tiny block member"));
        }
        let mut out =
            Vec::with_capacity(HEADER + self.entries.len() * MEMBER_HEADER + self.payload_bytes);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        let mut ids = Vec::with_capacity(self.entries.len());
        for (id, raw) in &self.entries {
            out.extend_from_slice(&id.0);
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            ids.push(*id);
        }
        for (_, raw) in self.entries.drain(..) {
            out.extend_from_slice(&raw);
        }
        self.payload_bytes = 0;
        Ok(Some((out, ids)))
    }
}
