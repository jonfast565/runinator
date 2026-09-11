//! Physical grouping for small logical metadata objects.

use crate::{
    Id,
    error::{Result, corrupt, invalid},
    model::Kind,
};

pub const BLOCK_LIMIT: usize = 512 * 1024;
const MAGIC: &[u8; 8] = b"PWMETA01";
const HEADER: usize = 12;
const MEMBER_HEADER: usize = 40;
const MAX_MEMBERS: usize = 8192;

pub fn eligible(kind: Kind, raw: &[u8]) -> bool {
    !matches!(
        kind,
        Kind::Chunk | Kind::Page | Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
    ) && raw.len().saturating_add(HEADER + MEMBER_HEADER) <= BLOCK_LIMIT
}

#[derive(Default)]
pub struct Pending {
    entries: Vec<(Id, Kind, Vec<u8>)>,
    payload_bytes: usize,
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

#[derive(Clone, Copy)]
pub struct Member<'a> {
    pub id: Id,
    pub kind: Kind,
    pub raw: &'a [u8],
}

pub fn table(raw: &[u8]) -> Result<Vec<Member<'_>>> {
    if raw.len() < HEADER || raw.len() > BLOCK_LIMIT || &raw[..8] != MAGIC {
        return Err(corrupt("invalid metadata block header/size"));
    }
    let count = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    if count == 0 || count > MAX_MEMBERS {
        return Err(corrupt("invalid metadata member count"));
    }
    let table_end = HEADER
        .checked_add(count.saturating_mul(MEMBER_HEADER))
        .ok_or_else(|| corrupt("metadata table overflow"))?;
    if table_end > raw.len() {
        return Err(corrupt("truncated metadata table"));
    }
    let mut previous = None;
    let mut expected_offset = table_end;
    let mut output = Vec::with_capacity(count);
    for slot in 0..count {
        let begin = HEADER + slot * MEMBER_HEADER;
        let id = Id(raw[begin..begin + 32].try_into().unwrap());
        let kind = Kind::try_from(raw[begin + 32])?;
        if raw[begin + 33..begin + 36] != [0; 3] || !eligible(kind, &[]) {
            return Err(corrupt("invalid metadata member kind/reserved bytes"));
        }
        let offset = u32::from_le_bytes(raw[begin + 36..begin + 40].try_into().unwrap()) as usize;
        let end = if slot + 1 == count {
            raw.len()
        } else {
            u32::from_le_bytes(
                raw[begin + MEMBER_HEADER + 36..begin + MEMBER_HEADER + 40]
                    .try_into()
                    .unwrap(),
            ) as usize
        };
        if previous.is_some_and(|prior| prior >= id) {
            return Err(corrupt("unordered metadata members"));
        }
        previous = Some(id);
        if offset != expected_offset || end < offset || end > raw.len() {
            return Err(corrupt("invalid metadata member offsets"));
        }
        let bytes = raw
            .get(offset..end)
            .ok_or_else(|| corrupt("metadata member out of bounds"))?;
        output.push(Member {
            id,
            kind,
            raw: bytes,
        });
        expected_offset = end;
    }
    if expected_offset != raw.len() {
        return Err(corrupt("trailing metadata block bytes"));
    }
    Ok(output)
}

pub fn verify(member: Member<'_>) -> Result<()> {
    if !eligible(member.kind, member.raw) || Id::object(member.kind, member.raw) != member.id {
        return Err(corrupt("invalid metadata member identity/representation"));
    }
    Ok(())
}

pub fn member(raw: &[u8], slot: u32, expected: Id) -> Result<Member<'_>> {
    if raw.len() < HEADER || raw.len() > BLOCK_LIMIT || &raw[..8] != MAGIC {
        return Err(corrupt("invalid metadata block header/size"));
    }
    let count = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    let slot = slot as usize;
    if count == 0 || count > MAX_MEMBERS || slot >= count {
        return Err(corrupt("metadata slot outside table"));
    }
    let table_end = HEADER
        .checked_add(count.saturating_mul(MEMBER_HEADER))
        .ok_or_else(|| corrupt("metadata table overflow"))?;
    if table_end > raw.len() {
        return Err(corrupt("truncated metadata table"));
    }
    let begin = HEADER + slot * MEMBER_HEADER;
    let id = Id(raw[begin..begin + 32].try_into().unwrap());
    let kind = Kind::try_from(raw[begin + 32])?;
    if raw[begin + 33..begin + 36] != [0; 3] || !eligible(kind, &[]) {
        return Err(corrupt("invalid metadata member kind/reserved bytes"));
    }
    let offset = u32::from_le_bytes(raw[begin + 36..begin + 40].try_into().unwrap()) as usize;
    let end = if slot + 1 == count {
        raw.len()
    } else {
        u32::from_le_bytes(
            raw[begin + MEMBER_HEADER + 36..begin + MEMBER_HEADER + 40]
                .try_into()
                .unwrap(),
        ) as usize
    };
    let bytes = raw
        .get(offset..end)
        .filter(|_| offset >= table_end)
        .ok_or_else(|| corrupt("metadata member out of bounds"))?;
    let entry = Member {
        id,
        kind,
        raw: bytes,
    };
    if entry.id != expected {
        return Err(corrupt("metadata index/member mismatch"));
    }
    verify(entry)?;
    Ok(entry)
}
