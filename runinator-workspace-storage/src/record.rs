//! Self-describing individually compressed records allow bounded random reads.
use crate::{
    Id,
    codec::MAX_OBJECT,
    error::{Result, corrupt, invalid},
    io_util::read_at,
    model::Kind,
    store::{Object, ObjectInfo},
};
use std::{fs::File, io::Write, sync::Arc};
#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
pub const HEADER_LEN: u64 = 96;
pub const PACK_MAGIC: &[u8; 8] = b"RNWPACK1";
const RECORD_MAGIC: &[u8; 8] = b"RNWREC01";

#[derive(Clone, Copy, Debug)]
pub struct PhysicalMemberInfo {
    pub id: Id,
    pub member: u32,
    pub kind: Kind,
    pub raw_len: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub id: Id,
    pub kind: Kind,
    pub codec: u8,
    pub raw_len: u64,
    pub encoded_len: u64,
    pub checksum: [u8; 32],
}
impl Header {
    pub fn record_len(&self) -> Result<u64> {
        HEADER_LEN
            .checked_add(self.encoded_len)
            .ok_or_else(|| corrupt("record length overflow"))
    }
    pub fn info(&self) -> ObjectInfo {
        ObjectInfo {
            kind: self.kind,
            raw_len: self.raw_len as usize,
        }
    }
    pub fn encode(&self) -> [u8; 96] {
        let mut b = [0; 96];
        b[..8].copy_from_slice(RECORD_MAGIC);
        b[8] = self.kind as u8;
        b[9] = self.codec;
        b[16..24].copy_from_slice(&self.raw_len.to_le_bytes());
        b[24..32].copy_from_slice(&self.encoded_len.to_le_bytes());
        b[32..64].copy_from_slice(&self.id.0);
        b[64..96].copy_from_slice(&self.checksum);
        b
    }
    pub fn decode(b: [u8; 96]) -> Result<Self> {
        if &b[..8] != RECORD_MAGIC || b[10..16] != [0; 6] {
            return Err(corrupt("bad record header"));
        }
        let x = Self {
            id: Id(b[32..64].try_into().unwrap()),
            kind: b[8].try_into()?,
            codec: b[9],
            raw_len: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            encoded_len: u64::from_le_bytes(b[24..32].try_into().unwrap()),
            checksum: b[64..96].try_into().unwrap(),
        };
        if x.codec > 1
            || x.raw_len > MAX_OBJECT as u64
            || x.encoded_len > MAX_OBJECT as u64 + 65536
            || (x.codec == 0 && x.raw_len != x.encoded_len)
            || (x.kind == Kind::TinyBlock
                && (x.raw_len > crate::tiny::BLOCK_LIMIT as u64
                    || x.encoded_len > crate::tiny::BLOCK_LIMIT as u64 + 65536))
            || (x.kind == Kind::MetadataBlock
                && (x.raw_len > crate::metadata_block::BLOCK_LIMIT as u64
                    || x.encoded_len > crate::metadata_block::BLOCK_LIMIT as u64 + 65536))
            || (x.kind == Kind::ChunkBlock && x.raw_len > crate::codec::MAX_OBJECT as u64)
        {
            return Err(corrupt("invalid record sizes/codec"));
        }
        Ok(x)
    }
}
pub fn header(file: &File, offset: u64) -> Result<Header> {
    let mut b = [0; 96];
    read_at(file, &mut b, offset)?;
    let h = Header::decode(b)?;
    let end = offset
        .checked_add(h.record_len()?)
        .ok_or_else(|| corrupt("record end overflow"))?;
    if end > file.metadata()?.len() {
        return Err(corrupt("record exceeds containing file"));
    }
    Ok(h)
}
pub fn read(file: &File, offset: u64, expected: Option<Id>) -> Result<(Header, Object)> {
    let h = header(file, offset)?;
    if expected.is_some_and(|id| id != h.id) {
        return Err(corrupt("record ID differs from index"));
    }
    let mut encoded = vec![0; h.encoded_len as usize];
    read_at(file, &mut encoded, offset + HEADER_LEN)?;
    let object = decode_payload(h, encoded)?;
    Ok((h, object))
}
fn decode_payload(h: Header, encoded: Vec<u8>) -> Result<Object> {
    if blake3::hash(&encoded).as_bytes() != &h.checksum {
        return Err(corrupt("encoded payload checksum mismatch"));
    }
    let raw = match h.codec {
        0 => encoded,
        1 => zstd::bulk::decompress(&encoded, h.raw_len as usize)?,
        _ => unreachable!(),
    };
    if raw.len() != h.raw_len as usize || Id::object(h.kind, &raw) != h.id {
        return Err(corrupt("decoded object identity mismatch"));
    }
    Ok(Object {
        kind: h.kind,
        bytes: Arc::new(raw),
    })
}
/// Decode exactly one ranged physical record and resolve an indexed logical member.
pub fn decode_range(bytes: &[u8], expected: Id, member: u32) -> Result<Object> {
    decode_ranges(bytes, &[(expected, member)])?
        .pop()
        .ok_or_else(|| corrupt("empty record range request"))
}

/// Decode one physical record once and resolve an ordered batch of indexed members.
pub fn decode_ranges(bytes: &[u8], members: &[(Id, u32)]) -> Result<Vec<Object>> {
    decode_ranges_inner(bytes, members, None)
}

/// Decode indexed members while retaining only bounded, request-local physical block payloads.
pub fn decode_ranges_cached(
    bytes: &[u8],
    members: &[(Id, u32)],
    decoded: &crate::cache::ByteCache,
) -> Result<Vec<Object>> {
    decode_ranges_inner(bytes, members, Some(decoded))
}

/// Read a physical metadata block's compact member index without hashing every logical object.
pub fn metadata_block_members_cached(
    bytes: &[u8],
    decoded: &crate::cache::ByteCache,
) -> Result<Option<Vec<PhysicalMemberInfo>>> {
    let header: [u8; 96] = bytes
        .get(..96)
        .ok_or_else(|| corrupt("truncated record"))?
        .try_into()
        .map_err(|_| corrupt("record header"))?;
    let h = Header::decode(header)?;
    if h.record_len()? != bytes.len() as u64 {
        return Err(corrupt("record range length mismatch"));
    }
    if h.kind != Kind::MetadataBlock {
        return Ok(None);
    }
    let key = Id::sha256(&h.encode());
    let raw = decoded.get_or_load(key, h.raw_len as usize, || {
        Ok((*decode_payload(h, bytes[96..].to_vec())?.bytes).clone())
    })?;
    let output = crate::metadata_block::table(&raw)?
        .into_iter()
        .enumerate()
        .map(|(slot, member)| PhysicalMemberInfo {
            id: member.id,
            member: slot as u32,
            kind: member.kind,
            raw_len: member.raw.len(),
        })
        .collect();
    Ok(Some(output))
}

fn decode_ranges_inner(
    bytes: &[u8],
    members: &[(Id, u32)],
    decoded: Option<&crate::cache::ByteCache>,
) -> Result<Vec<Object>> {
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let header: [u8; 96] = bytes
        .get(..96)
        .ok_or_else(|| corrupt("truncated record"))?
        .try_into()
        .map_err(|_| corrupt("record header"))?;
    let h = Header::decode(header)?;
    if h.record_len()? != bytes.len() as u64 {
        return Err(corrupt("record range length mismatch"));
    }
    let object = if matches!(
        h.kind,
        Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
    ) && let Some(decoded) = decoded
    {
        let key = Id::sha256(&h.encode());
        let raw = decoded.get_or_load(key, h.raw_len as usize, || {
            Ok((*decode_payload(h, bytes[96..].to_vec())?.bytes).clone())
        })?;
        Object {
            kind: h.kind,
            bytes: raw,
        }
    } else {
        decode_payload(h, bytes[96..].to_vec())?
    };
    resolve_members(h, object, members)
}

/// Read an indexed member while verifying and decoding its shared physical container once.
pub fn read_indexed(
    file: &File,
    location: crate::index::Location,
    cache: &crate::cache::ByteCache,
) -> Result<Object> {
    let h = header(file, location.offset)?;
    if h.record_len()? != location.length {
        return Err(corrupt("record range length mismatch"));
    }
    let object = if location.member == crate::index::STANDALONE {
        read(file, location.offset, Some(location.id))?.1
    } else {
        // bind the cached decoded bytes to every validated physical header field.
        let key = Id::sha256(&h.encode());
        let bytes = cache.get_or_load(key, h.raw_len as usize, || {
            let (_, object) = read(file, location.offset, Some(h.id))?;
            Ok((*object.bytes).clone())
        })?;
        Object {
            kind: h.kind,
            bytes,
        }
    };
    resolve_member(h, object, location.id, location.member)
}

fn resolve_member(h: Header, object: Object, expected: Id, member: u32) -> Result<Object> {
    resolve_members(h, object, &[(expected, member)])?
        .pop()
        .ok_or_else(|| corrupt("empty record member request"))
}

fn resolve_members(h: Header, object: Object, members: &[(Id, u32)]) -> Result<Vec<Object>> {
    if members
        .iter()
        .any(|(_, member)| *member == crate::index::STANDALONE)
    {
        if members.len() != 1
            || members[0].1 != crate::index::STANDALONE
            || h.id != members[0].0
            || matches!(
                h.kind,
                Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
            )
        {
            return Err(corrupt("logical record mismatch"));
        }
        return Ok(vec![object]);
    }
    match h.kind {
        Kind::TinyBlock => {
            let table = crate::tiny::table(&object.bytes)?;
            members
                .iter()
                .map(|(expected, slot)| {
                    let item = *table
                        .get(*slot as usize)
                        .ok_or_else(|| corrupt("tiny slot outside table"))?;
                    if item.id != *expected {
                        return Err(corrupt("tiny index/member mismatch"));
                    }
                    crate::tiny::verify(item)?;
                    Ok(Object {
                        kind: Kind::File,
                        bytes: Arc::new(item.raw.to_vec()),
                    })
                })
                .collect()
        }
        Kind::ChunkBlock => {
            let table = crate::chunkblock::table(&object.bytes)?;
            members
                .iter()
                .map(|(expected, slot)| {
                    let item = table
                        .get(*slot as usize)
                        .ok_or_else(|| corrupt("chunk slot outside table"))?;
                    if item.id != *expected {
                        return Err(corrupt("chunk index/member mismatch"));
                    }
                    Ok(Object {
                        kind: Kind::Chunk,
                        bytes: Arc::new(item.raw.clone()),
                    })
                })
                .collect()
        }
        Kind::MetadataBlock => members
            .iter()
            .map(|(expected, slot)| {
                let item = crate::metadata_block::member(&object.bytes, *slot, *expected)?;
                Ok(Object {
                    kind: item.kind,
                    bytes: Arc::new(item.raw.to_vec()),
                })
            })
            .collect(),
        _ => Err(corrupt("member record is not a physical container")),
    }
}
pub fn write<W: Write>(writer: &mut W, kind: Kind, raw: &[u8]) -> Result<(Id, u64)> {
    if raw.len() > MAX_OBJECT
        || (kind == Kind::TinyBlock && raw.len() > crate::tiny::BLOCK_LIMIT)
        || (kind == Kind::MetadataBlock && raw.len() > crate::metadata_block::BLOCK_LIMIT)
    {
        return Err(invalid("object exceeds decoded size limit"));
    }
    let id = Id::object(kind, raw);
    let compressed =
        if matches!(kind, Kind::Chunk | Kind::TinyBlock | Kind::MetadataBlock) && !raw.is_empty() {
            Some(zstd::bulk::compress(raw, 3)?)
        } else {
            None
        };
    let (codec, payload) = match &compressed {
        Some(c) if c.len() < raw.len() => (1, c.as_slice()),
        _ => (0, raw),
    };
    let h = Header {
        id,
        kind,
        codec,
        raw_len: raw.len() as u64,
        encoded_len: payload.len() as u64,
        checksum: *blake3::hash(payload).as_bytes(),
    };
    writer.write_all(&h.encode())?;
    writer.write_all(payload)?;
    Ok((id, h.record_len()?))
}
pub fn visit_pack<F: FnMut(Id, Object) -> Result<()>>(
    path: &std::path::Path,
    mut visit: F,
) -> Result<u64> {
    let file = File::open(path)?;
    let mut magic = [0; 8];
    read_at(&file, &mut magic, 0)?;
    if &magic != PACK_MAGIC {
        return Err(corrupt("invalid pack magic"));
    }
    let size = file.metadata()?.len();
    let mut pos = 8;
    let mut count = 0;
    while pos < size {
        let (h, object) = read(&file, pos, None)?;
        if object.kind == Kind::TinyBlock {
            for member in crate::tiny::table(&object.bytes)? {
                crate::tiny::verify(member)?;
                visit(
                    member.id,
                    Object {
                        kind: Kind::File,
                        bytes: Arc::new(member.raw.to_vec()),
                    },
                )?;
                count += 1;
            }
        } else if object.kind == Kind::ChunkBlock {
            for member in crate::chunkblock::table(&object.bytes)? {
                visit(
                    member.id,
                    Object {
                        kind: Kind::Chunk,
                        bytes: Arc::new(member.raw),
                    },
                )?;
                count += 1;
            }
        } else if object.kind == Kind::MetadataBlock {
            for member in crate::metadata_block::table(&object.bytes)? {
                crate::metadata_block::verify(member)?;
                visit(
                    member.id,
                    Object {
                        kind: member.kind,
                        bytes: Arc::new(member.raw.to_vec()),
                    },
                )?;
                count += 1;
            }
        } else {
            visit(h.id, object)?;
            count += 1;
        }
        pos += h.record_len()?;
    }
    Ok(count)
}

/// Validate every physical member and stream its server-derived location.
pub fn index_pack<F: FnMut(crate::index::Location, ObjectInfo) -> Result<()>>(
    path: &std::path::Path,
    pack: Id,
    mut visit: F,
) -> Result<()> {
    let file = File::open(path)?;
    let mut magic = [0; 8];
    read_at(&file, &mut magic, 0)?;
    if &magic != PACK_MAGIC {
        return Err(corrupt("invalid pack magic"));
    }
    let size = file.metadata()?.len();
    let mut offset = 8;
    while offset < size {
        let (header, object) = read(&file, offset, None)?;
        let length = header.record_len()?;
        match object.kind {
            Kind::TinyBlock => {
                for (slot, item) in crate::tiny::table(&object.bytes)?.into_iter().enumerate() {
                    crate::tiny::verify(item)?;
                    visit(
                        crate::index::Location {
                            id: item.id,
                            pack,
                            offset,
                            length,
                            member: slot as u32,
                        },
                        ObjectInfo {
                            kind: Kind::File,
                            raw_len: item.raw.len(),
                        },
                    )?;
                }
            }
            Kind::ChunkBlock => {
                for (slot, item) in crate::chunkblock::table(&object.bytes)?
                    .into_iter()
                    .enumerate()
                {
                    visit(
                        crate::index::Location {
                            id: item.id,
                            pack,
                            offset,
                            length,
                            member: slot as u32,
                        },
                        ObjectInfo {
                            kind: Kind::Chunk,
                            raw_len: item.raw.len(),
                        },
                    )?;
                }
            }
            Kind::MetadataBlock => {
                for (slot, item) in crate::metadata_block::table(&object.bytes)?
                    .into_iter()
                    .enumerate()
                {
                    crate::metadata_block::verify(item)?;
                    visit(
                        crate::index::Location {
                            id: item.id,
                            pack,
                            offset,
                            length,
                            member: slot as u32,
                        },
                        ObjectInfo {
                            kind: item.kind,
                            raw_len: item.raw.len(),
                        },
                    )?;
                }
            }
            _ => visit(
                crate::index::Location {
                    id: header.id,
                    pack,
                    offset,
                    length,
                    member: crate::index::STANDALONE,
                },
                header.info(),
            )?,
        }
        offset = offset
            .checked_add(length)
            .ok_or_else(|| corrupt("pack offset overflow"))?;
    }
    if offset != size {
        return Err(corrupt("pack record exceeds file length"));
    }
    Ok(())
}
