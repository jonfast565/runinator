#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub struct Pending {
    pub(super) members: Vec<(Id, Vec<u8>)>,
    pub(super) bytes: usize,
}

impl Pending {
    pub fn would_overflow(&self, n: usize) -> bool {
        !self.members.is_empty()
            && (self.bytes.saturating_add(n) > BLOCK_LIMIT || self.members.len() >= MAX_MEMBERS)
    }
    pub fn push(&mut self, id: Id, raw: &[u8]) -> Result<()> {
        if raw.is_empty() || raw.len() > crate::codec::MAX_OBJECT {
            return Err(invalid("invalid chunk block member"));
        }
        self.bytes = self
            .bytes
            .checked_add(raw.len())
            .ok_or_else(|| invalid("chunk group size overflow"))?;
        self.members.push((id, raw.to_vec()));
        Ok(())
    }
    pub fn take(&mut self) -> Result<Option<(Vec<u8>, Vec<Id>)>> {
        let Some(members) = self.take_members() else {
            return Ok(None);
        };
        encode(&members).map(Some)
    }

    pub(crate) fn take_members(&mut self) -> Option<Vec<(Id, Vec<u8>)>> {
        if self.members.is_empty() {
            return None;
        }
        self.bytes = 0;
        Some(std::mem::take(&mut self.members))
    }
}
