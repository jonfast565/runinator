//! Physical grouping of small FileObjects. No logical object ever refers to a
//! block: the physical index routes each stable File ID to a block member.
use crate::{
    Id,
    codec::Binary,
    error::{Result, corrupt, invalid},
    model::{FileObject, Kind},
};

pub const BLOCK_LIMIT: usize = 256 * 1024;
pub const BLOCK_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"PWTINY01";
const HEADER: usize = 12;
const MEMBER_HEADER: usize = 36;
const MAX_MEMBERS: usize = 4096;

pub fn eligible(kind: Kind, raw: &[u8]) -> Result<bool> {
    if kind != Kind::File {
        return Ok(false);
    }
    Ok(FileObject::decode(raw)?.is_tiny())
}

#[derive(Default)]
pub struct Pending {
    entries: Vec<(Id, Vec<u8>)>,
    payload_bytes: usize,
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

#[derive(Clone, Copy)]
pub struct Member<'a> {
    pub id: Id,
    pub raw: &'a [u8],
}
/// Validate the bounded table and every range, without copying member payloads.
pub fn table(raw: &[u8]) -> Result<Vec<Member<'_>>> {
    if raw.len() < HEADER || raw.len() > BLOCK_LIMIT || &raw[..8] != MAGIC {
        return Err(corrupt("invalid tiny block header/size"));
    }
    let count = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    if count == 0 || count > MAX_MEMBERS {
        return Err(corrupt("invalid tiny member count"));
    }
    let mut offset = HEADER + count * MEMBER_HEADER;
    if offset > raw.len() {
        return Err(corrupt("truncated tiny table"));
    }
    let mut previous = None;
    let mut out = Vec::with_capacity(count);
    for slot in 0..count {
        let begin = HEADER + slot * MEMBER_HEADER;
        let id = Id(raw[begin..begin + 32].try_into().unwrap());
        let len = u32::from_le_bytes(raw[begin + 32..begin + 36].try_into().unwrap()) as usize;
        if previous.is_some_and(|p| p >= id) {
            return Err(corrupt("unordered tiny members"));
        }
        previous = Some(id);
        let end = offset
            .checked_add(len)
            .ok_or_else(|| corrupt("tiny member overflow"))?;
        let bytes = raw
            .get(offset..end)
            .ok_or_else(|| corrupt("tiny member out of bounds"))?;
        out.push(Member { id, raw: bytes });
        offset = end;
    }
    if offset != raw.len() {
        return Err(corrupt("trailing tiny block bytes"));
    }
    Ok(out)
}

pub fn verify(member: Member<'_>) -> Result<()> {
    if Id::object(Kind::File, member.raw) != member.id || !eligible(Kind::File, member.raw)? {
        return Err(corrupt("invalid tiny member identity/representation"));
    }
    Ok(())
}

pub fn member(raw: &[u8], slot: u32, expected: Id) -> Result<Member<'_>> {
    let entries = table(raw)?;
    let entry = *entries
        .get(slot as usize)
        .ok_or_else(|| corrupt("tiny slot outside table"))?;
    if entry.id != expected {
        return Err(corrupt("tiny index/member mismatch"));
    }
    verify(entry)?;
    Ok(entry)
}
