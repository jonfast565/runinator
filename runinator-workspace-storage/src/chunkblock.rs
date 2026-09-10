//! Physical-only compression groups for logical Chunk objects.
//!
//! A block groups several FastCDC chunks, builds one shared zstd content
//! dictionary, and optionally stores a member as a shallow prefix/suffix delta
//! against an earlier *full* member selected by a small similarity index.
//! Logical Chunk IDs remain typed BLAKE3(raw chunk bytes); this module only
//! changes physical representation.
use crate::{
    Id,
    error::{Result, corrupt, invalid},
};
use std::collections::BTreeMap;

pub const BLOCK_LIMIT: usize = 4 * 1024 * 1024;
pub const DICT_LIMIT: usize = 32 * 1024;
pub const MAX_MEMBERS: usize = 256;
const MAGIC: &[u8; 8] = b"PWCHNK01";
const RAW: u8 = 0;
const ZSTD_DICT: u8 = 1;
const DELTA_DICT: u8 = 2;
const NONE_BASE: u32 = u32::MAX;
const MIN_DELTA_SAVING: usize = 64;

#[derive(Clone, Debug)]
pub struct Member {
    pub id: Id,
    pub raw: Vec<u8>,
    #[cfg(test)]
    pub is_delta: bool,
}

#[derive(Default)]
pub struct Pending {
    members: Vec<(Id, Vec<u8>)>,
    bytes: usize,
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
        if self.members.is_empty() {
            return Ok(None);
        }
        let members = std::mem::take(&mut self.members);
        self.bytes = 0;
        encode(&members).map(Some)
    }
}

fn dictionary(members: &[(Id, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DICT_LIMIT);
    for (_, raw) in members {
        if out.len() >= DICT_LIMIT {
            break;
        }
        // Take both head and tail so common headers and recurring suffix syntax
        // can participate without retaining a separate training corpus.
        let budget = (DICT_LIMIT - out.len()).min(4096);
        let head = budget.div_ceil(2);
        out.extend_from_slice(&raw[..head.min(raw.len())]);
        let remain = budget.saturating_sub(head.min(raw.len()));
        if remain > 0 && raw.len() > head {
            out.extend_from_slice(&raw[raw.len() - remain.min(raw.len() - head)..]);
        }
    }
    out
}

fn sketch(raw: &[u8]) -> u64 {
    // Four 16-bit minhash lanes over 64-byte samples. Small, deterministic and
    // sufficient for choosing candidates inside a bounded compression group.
    let mut mins = [u16::MAX; 4];
    if raw.is_empty() {
        return 0;
    }
    let step = 64usize;
    for (i, s) in raw.chunks(step).enumerate() {
        let h = blake3::hash(s);
        let b = h.as_bytes();
        let lane = i & 3;
        let v = u16::from_le_bytes([b[lane * 2], b[lane * 2 + 1]]);
        mins[lane] = mins[lane].min(v);
    }
    (mins[0] as u64)
        | ((mins[1] as u64) << 16)
        | ((mins[2] as u64) << 32)
        | ((mins[3] as u64) << 48)
}
fn similarity(a: u64, b: u64) -> u32 {
    let mut n = 0;
    for shift in [0, 16, 32, 48] {
        if ((a >> shift) & 0xffff) == ((b >> shift) & 0xffff) {
            n += 1;
        }
    }
    n
}
fn delta(base: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut prefix = 0usize;
    let m = base.len().min(raw.len());
    while prefix < m && base[prefix] == raw[prefix] {
        prefix += 1
    }
    let mut suffix = 0usize;
    while suffix < m - prefix && base[base.len() - 1 - suffix] == raw[raw.len() - 1 - suffix] {
        suffix += 1
    }
    let middle = &raw[prefix..raw.len() - suffix];
    let mut d = Vec::with_capacity(8 + middle.len());
    d.extend_from_slice(&(prefix as u32).to_le_bytes());
    d.extend_from_slice(&(suffix as u32).to_le_bytes());
    d.extend_from_slice(middle);
    d
}
fn apply_delta(base: &[u8], delta: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    if delta.len() < 8 {
        return Err(corrupt("truncated chunk delta"));
    }
    let p = u32::from_le_bytes(delta[..4].try_into().unwrap()) as usize;
    let s = u32::from_le_bytes(delta[4..8].try_into().unwrap()) as usize;
    if p > base.len() || s > base.len().saturating_sub(p) {
        return Err(corrupt("invalid chunk delta base ranges"));
    }
    let mid = &delta[8..];
    if p.checked_add(mid.len()).and_then(|n| n.checked_add(s)) != Some(raw_len) {
        return Err(corrupt("chunk delta length mismatch"));
    }
    let mut out = Vec::with_capacity(raw_len);
    out.extend_from_slice(&base[..p]);
    out.extend_from_slice(mid);
    out.extend_from_slice(&base[base.len() - s..]);
    Ok(out)
}

fn zcompress(dict: &[u8], raw: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::bulk::Compressor::with_dictionary(3, dict)?.compress(raw)?)
}
fn zdecompress(dict: &[u8], encoded: &[u8], limit: usize) -> Result<Vec<u8>> {
    Ok(zstd::bulk::Decompressor::with_dictionary(dict)?.decompress(encoded, limit)?)
}

pub fn encode(members: &[(Id, Vec<u8>)]) -> Result<(Vec<u8>, Vec<Id>)> {
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return Err(invalid("invalid chunk group member count"));
    }
    let dict = dictionary(members);
    let mut records: Vec<(Id, u32, u8, u32, Vec<u8>)> = Vec::with_capacity(members.len());
    let mut buckets: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    let mut full_raw: Vec<Option<&[u8]>> = Vec::new();
    let mut full_sketch = Vec::new();
    for (idx, (id, raw)) in members.iter().enumerate() {
        let full = zcompress(&dict, raw)?;
        let sig = sketch(raw);
        let mut best: Option<(usize, Vec<u8>)> = None;
        // Candidate index: any shared 16-bit minhash lane. Evaluate at most 24
        // recent full candidates, favoring those with more matching lanes.
        let mut candidates = Vec::new();
        for shift in [0, 16, 32, 48] {
            let Some(v) = buckets.get(&(((sig >> shift) & 0xffff) as u16)) else {
                continue;
            };

            for &c in v.iter().rev().take(6) {
                if !candidates.contains(&c) {
                    candidates.push(c)
                }
            }
        }
        candidates.sort_by_key(|&c| std::cmp::Reverse(similarity(sig, full_sketch[c])));
        for c in candidates.into_iter().take(24) {
            let Some(base) = full_raw[c] else { continue };
            let d = delta(base, raw);
            let enc = zcompress(&dict, &d)?;
            if enc.len() + MIN_DELTA_SAVING < full.len()
                && best.as_ref().is_none_or(|(_, b)| enc.len() < b.len())
            {
                best = Some((c, enc));
            }
        }
        let (codec, base, payload) = if let Some((c, d)) = best {
            (DELTA_DICT, c as u32, d)
        } else if full.len() < raw.len() {
            (ZSTD_DICT, NONE_BASE, full)
        } else {
            (RAW, NONE_BASE, raw.clone())
        };
        let is_full = codec != DELTA_DICT;
        records.push((*id, raw.len() as u32, codec, base, payload));
        full_raw.push(if is_full { Some(raw.as_slice()) } else { None });
        full_sketch.push(sig);
        if is_full {
            for shift in [0, 16, 32, 48] {
                buckets
                    .entry(((sig >> shift) & 0xffff) as u16)
                    .or_default()
                    .push(idx);
            }
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(dict.len() as u32).to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    out.extend_from_slice(&dict);
    for (id, raw_len, codec, base, payload) in &records {
        out.extend_from_slice(&id.0);
        out.extend_from_slice(&raw_len.to_le_bytes());
        out.push(*codec);
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&base.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
    }
    if out.len() > crate::codec::MAX_OBJECT {
        return Err(invalid("chunk compression group exceeds object limit"));
    }
    Ok((out, members.iter().map(|m| m.0).collect()))
}

struct Meta<'a> {
    id: Id,
    raw_len: usize,
    codec: u8,
    base: u32,
    payload: &'a [u8],
}
fn parse(raw: &[u8]) -> Result<(Vec<u8>, Vec<Meta<'_>>)> {
    if raw.len() < 16 || &raw[..8] != MAGIC {
        return Err(corrupt("bad chunk block"));
    }
    let dl = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
    if n == 0 || n > MAX_MEMBERS || dl > DICT_LIMIT || 16 + dl > raw.len() {
        return Err(corrupt("invalid chunk block header"));
    }
    let dict = raw[16..16 + dl].to_vec();
    let mut pos = 16 + dl;
    let mut metas = Vec::with_capacity(n);
    for i in 0..n {
        if pos + 48 > raw.len() {
            return Err(corrupt("truncated chunk block member"));
        }
        let id = Id(raw[pos..pos + 32].try_into().unwrap());
        pos += 32;
        let raw_len = u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let codec = raw[pos];
        if raw[pos + 1..pos + 4] != [0; 3] {
            return Err(corrupt("nonzero chunk member reserved bytes"));
        }
        pos += 4;
        let base = u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let plen = u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + plen > raw.len()
            || raw_len == 0
            || raw_len > crate::codec::MAX_OBJECT
            || codec > DELTA_DICT
            || (codec == RAW && raw_len != plen)
            || (codec != DELTA_DICT && base != NONE_BASE)
        {
            return Err(corrupt("invalid chunk block member"));
        }
        if codec == DELTA_DICT && base as usize >= i {
            return Err(corrupt("delta base is not an earlier full member"));
        }
        metas.push(Meta {
            id,
            raw_len,
            codec,
            base,
            payload: &raw[pos..pos + plen],
        });
        pos += plen;
    }
    if n > 1 && metas.iter().map(|m| m.raw_len).sum::<usize>() > BLOCK_LIMIT {
        return Err(corrupt("chunk group exceeds decoded budget"));
    }
    if pos != raw.len() {
        return Err(corrupt("trailing chunk block bytes"));
    }
    Ok((dict, metas))
}

pub fn info(raw: &[u8], slot: u32, expected: Id) -> Result<usize> {
    let (_, m) = parse(raw)?;
    let x = m
        .get(slot as usize)
        .ok_or_else(|| corrupt("chunk block slot"))?;
    if x.id != expected {
        return Err(corrupt("chunk block member ID mismatch"));
    }
    Ok(x.raw_len)
}
pub fn member(raw: &[u8], slot: u32, expected: Id) -> Result<Member> {
    let (dict, m) = parse(raw)?;
    let i = slot as usize;
    let x = m.get(i).ok_or_else(|| corrupt("chunk block slot"))?;
    if x.id != expected {
        return Err(corrupt("chunk block member ID mismatch"));
    }
    let bytes = match x.codec {
        RAW => x.payload.to_vec(),
        ZSTD_DICT => zdecompress(&dict, x.payload, x.raw_len)?,
        DELTA_DICT => {
            let b = m
                .get(x.base as usize)
                .ok_or_else(|| corrupt("missing chunk delta base"))?;
            if b.codec == DELTA_DICT {
                return Err(corrupt("delta chain exceeds depth one"));
            }
            let base = match b.codec {
                RAW => b.payload.to_vec(),
                ZSTD_DICT => zdecompress(&dict, b.payload, b.raw_len)?,
                _ => unreachable!(),
            };
            if base.len() != b.raw_len || Id::object(crate::model::Kind::Chunk, &base) != b.id {
                return Err(corrupt("chunk delta base identity mismatch"));
            }
            let d = zdecompress(&dict, x.payload, x.raw_len.saturating_add(8))?;
            apply_delta(&base, &d, x.raw_len)?
        }
        _ => unreachable!(),
    };
    if bytes.len() != x.raw_len || Id::object(crate::model::Kind::Chunk, &bytes) != x.id {
        return Err(corrupt("decoded chunk block member identity mismatch"));
    }
    Ok(Member {
        id: x.id,
        raw: bytes,
        #[cfg(test)]
        is_delta: x.codec == DELTA_DICT,
    })
}

pub fn table(raw: &[u8]) -> Result<Vec<Member>> {
    let (_, m) = parse(raw)?;
    (0..m.len())
        .map(|i| member(raw, i as u32, m[i].id))
        .collect()
}

#[cfg(test)]
#[path = "chunkblock_tests.rs"]
mod tests;
