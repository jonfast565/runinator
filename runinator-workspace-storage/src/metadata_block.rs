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

mod pending;
pub use pending::Pending;

mod member;
pub use member::Member;
