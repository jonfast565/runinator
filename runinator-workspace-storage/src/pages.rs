//! Logical pages are independent of storage chunks. Range writes OVERWRITE;
//! inserting bytes is not a cheap/local operation in a fixed-page layout.
use crate::{
    Id,
    cache::ByteCache,
    error::{Result, corrupt, invalid},
    model::{ChunkRef, FileObject, Kind, Layout, Page, PageExtent, TINY_LIMIT, ZERO_RUN_MIN},
    radix,
    store::{ReadStore, WriteStore, load, save},
};
use fastcdc::v2020::{FastCDC, Normalization};
use rayon::prelude::*;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    sync::Arc,
};
fn chunk_data<S: WriteStore + ?Sized>(
    s: &S,
    layout: Layout,
    bytes: &[u8],
    out: &mut Vec<PageExtent>,
) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let chunks = FastCDC::with_level_and_seed(
        bytes,
        layout.min as usize,
        layout.avg as usize,
        layout.max as usize,
        Normalization::Level1,
        0,
    )
    .map(|chunk| &bytes[chunk.offset..chunk.offset + chunk.length])
    .collect::<Vec<_>>();
    let stored = if chunks.len() < 8 {
        chunks
            .iter()
            .map(|raw| Ok((s.put(Kind::Chunk, raw)?, raw.len())))
            .collect::<Result<Vec<_>>>()?
    } else {
        chunks
            .par_iter()
            .map(|raw| Ok((s.put(Kind::Chunk, raw)?, raw.len())))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>>>()?
    };
    for (id, len) in stored {
        out.push(PageExtent::Data(ChunkRef {
            id,
            len: len as u32,
        }));
    }
    Ok(())
}

pub fn store_page<S: WriteStore + ?Sized>(
    s: &S,
    layout: Layout,
    bytes: &[u8],
) -> Result<Option<Id>> {
    layout.validate()?;
    if bytes.len() > layout.page_size as usize {
        return Err(invalid("page too large"));
    }
    let Some(last) = bytes.iter().rposition(|&b| b != 0) else {
        return Ok(None);
    };
    let bytes = &bytes[..=last];
    let mut extents = Vec::new();
    let mut segment = 0;
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] != 0 {
            pos += 1;
            continue;
        }
        let begin = pos;
        while pos < bytes.len() && bytes[pos] == 0 {
            pos += 1;
        }
        if pos - begin < ZERO_RUN_MIN {
            continue;
        }
        chunk_data(s, layout, &bytes[segment..begin], &mut extents)?;
        extents.push(PageExtent::Zero((pos - begin) as u32));
        segment = pos;
    }
    chunk_data(s, layout, &bytes[segment..], &mut extents)?;
    Ok(Some(save(
        s,
        Kind::Page,
        &Page {
            page_size: layout.page_size,
            used: bytes.len() as u32,
            extents,
        },
    )?))
}

pub fn pin_page<S: ReadStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    id: Id,
    page_size: u32,
) -> Result<Arc<Vec<u8>>> {
    // Validate even on cache hits; malformed sizes must not alias a cached page.
    if !page_size.is_power_of_two() || !(4096..=8 * 1024 * 1024).contains(&page_size) {
        return Err(corrupt("invalid requested page size"));
    }
    let result = cache.get_or_load(id, page_size as usize, || {
        let p: Page = load(s, id, Kind::Page)?;
        if p.page_size != page_size {
            return Err(corrupt("page-size mismatch"));
        }
        let mut result = vec![0; page_size as usize];
        let mut positions = Vec::new();
        let mut chunks = Vec::new();
        let mut pos = 0usize;
        for extent in p.extents {
            let end = pos
                .checked_add(extent.len() as usize)
                .ok_or_else(|| corrupt("extent overflow"))?;
            if end > result.len() {
                return Err(corrupt("extent outside page"));
            }
            if let PageExtent::Data(c) = extent {
                chunks.push(c.id);
                positions.push((pos, end, c));
            }
            pos = end;
        }
        if pos != p.used as usize || pos == 0 {
            return Err(corrupt("invalid page length"));
        }
        let objects = s.get_many(&chunks)?;
        if objects.len() != chunks.len() {
            return Err(corrupt("bulk read returned incorrect object count"));
        }
        for ((begin, end, chunk), object) in positions.into_iter().zip(objects) {
            if object.kind != Kind::Chunk || object.bytes.len() != chunk.len as usize {
                return Err(corrupt("invalid chunk reference"));
            }
            result[begin..end].copy_from_slice(&object.bytes);
        }
        if result[pos - 1] == 0 {
            return Err(corrupt("noncanonical page padding"));
        }
        Ok(result)
    })?;
    if result.len() != page_size as usize {
        return Err(corrupt("cached page-size mismatch"));
    }
    Ok(result)
}
pub fn page_id<S: ReadStore + ?Sized>(s: &S, file: Id, page: u64) -> Result<Option<Id>> {
    let f: FileObject = load(s, file, Kind::File)?;
    radix::get(s, f.pages, &page.to_be_bytes())
}
pub fn empty_file<S: WriteStore + ?Sized>(s: &S, layout: Layout) -> Result<Id> {
    save(s, Kind::File, &FileObject::from_small(layout, Vec::new())?)
}

/// Bounded look-ahead distinguishes small files without needing a length hint.
pub fn ingest<S: WriteStore + ?Sized, R: Read>(s: &S, layout: Layout, mut source: R) -> Result<Id> {
    layout.validate()?;
    let mut prefix = vec![0; TINY_LIMIT + 1];
    let used = fill(&mut source, &mut prefix)?;
    prefix.truncate(used);
    if used <= TINY_LIMIT {
        return save(s, Kind::File, &FileObject::from_small(layout, prefix)?);
    }
    ingest_paged(s, layout, std::io::Cursor::new(prefix).chain(source))
}

fn fill<R: Read>(source: &mut R, buffer: &mut [u8]) -> Result<usize> {
    let mut used = 0;
    while used < buffer.len() {
        match source.read(&mut buffer[used..]) {
            Ok(0) => break,
            Ok(n) => used += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(used)
}

/// Explicit fixed-page ingestion, including for small inputs. Useful when a
/// caller needs page-sharing behavior regardless of the tiny-file policy.
pub fn ingest_paged<S: WriteStore + ?Sized, R: Read>(
    s: &S,
    layout: Layout,
    mut source: R,
) -> Result<Id> {
    layout.validate()?;
    let mut f = FileObject::paged(0, layout, None);
    let mut buffer = vec![0; layout.page_size as usize];
    let mut page = 0u64;
    loop {
        let used = fill(&mut source, &mut buffer)?;
        if used == 0 {
            break;
        }
        let id = store_page(s, layout, &buffer[..used])?;
        if id.is_some() {
            f.pages = radix::set(s, f.pages, &page.to_be_bytes(), id)?;
        }
        f.size = f
            .size
            .checked_add(used as u64)
            .ok_or_else(|| invalid("file too large"))?;
        page = page
            .checked_add(1)
            .ok_or_else(|| invalid("page number overflow"))?;
        if used < buffer.len() {
            break;
        }
    }
    save(s, Kind::File, &f)
}
pub fn read_into<S: ReadStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    offset: u64,
    output: &mut [u8],
) -> Result<usize> {
    let f: FileObject = load(s, file, Kind::File)?;
    read_into_file(s, cache, &f, offset, output)
}

fn read_into_file<S: ReadStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    f: &FileObject,
    offset: u64,
    output: &mut [u8],
) -> Result<usize> {
    if offset >= f.size || output.is_empty() {
        return Ok(0);
    }
    let length = (f.size - offset).min(output.len() as u64) as usize;
    if let Some(bytes) = &f.small {
        output[..length].copy_from_slice(&bytes[offset as usize..offset as usize + length]);
        return Ok(length);
    }
    let mut done = 0;
    while done < length {
        let pos = offset + done as u64;
        let no = pos / f.layout.page_size as u64;
        let inside = (pos % f.layout.page_size as u64) as usize;
        let take = (length - done).min(f.layout.page_size as usize - inside);
        let dst = &mut output[done..done + take];
        match radix::get(s, f.pages, &no.to_be_bytes())? {
            None => dst.fill(0),
            Some(id) => {
                let bytes = pin_page(s, cache, id, f.layout.page_size)?;
                dst.copy_from_slice(&bytes[inside..inside + take]);
            }
        }
        done += take;
    }
    Ok(done)
}
/// Convenience only. Use read_into/copy_to for arbitrary-sized output.
pub fn read_range<S: ReadStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>> {
    if length > 64 * 1024 * 1024 {
        return Err(invalid(
            "read_range is limited to 64 MiB; use streaming read_into",
        ));
    }
    let mut bytes = vec![0; length];
    let n = read_into(s, cache, file, offset, &mut bytes)?;
    bytes.truncate(n);
    Ok(bytes)
}
pub fn copy_to<S: ReadStore + ?Sized, W: Write>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    mut writer: W,
) -> Result<u64> {
    let f: FileObject = load(s, file, Kind::File)?;
    let mut buf = vec![0; f.size.min(f.layout.page_size as u64) as usize];
    let mut pos = 0u64;
    while pos < f.size {
        let n = read_into_file(s, cache, &f, pos, &mut buf)?;
        if n == 0 {
            return Err(corrupt("unexpected EOF"));
        }
        writer.write_all(&buf[..n])?;
        pos += n as u64;
    }
    Ok(pos)
}
fn mutable_page<S: ReadStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    f: &FileObject,
    no: u64,
) -> Result<Vec<u8>> {
    match radix::get(s, f.pages, &no.to_be_bytes())? {
        None => Ok(vec![0; f.layout.page_size as usize]),
        Some(id) => Ok((*pin_page(s, cache, id, f.layout.page_size)?).clone()),
    }
}
/// Convert a small immutable leaf to pages only when it actually outgrows it.
fn promote<S: WriteStore + ?Sized>(s: &S, f: &mut FileObject) -> Result<()> {
    let Some(bytes) = f.small.take() else {
        return Ok(());
    };
    for (no, bytes) in bytes.chunks(f.layout.page_size as usize).enumerate() {
        let page = store_page(s, f.layout, bytes)?;
        if page.is_some() {
            f.pages = radix::set(s, f.pages, &(no as u64).to_be_bytes(), page)?;
        }
    }
    Ok(())
}

pub fn write_range<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    offset: u64,
    input: &[u8],
) -> Result<Id> {
    if input.is_empty() {
        return Ok(file);
    }
    let mut f: FileObject = load(s, file, Kind::File)?;
    write_range_file(s, cache, &mut f, offset, input)?;
    save(s, Kind::File, &f)
}

fn write_range_file<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    f: &mut FileObject,
    offset: u64,
    input: &[u8],
) -> Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    let end = offset
        .checked_add(input.len() as u64)
        .ok_or_else(|| invalid("write offset overflow"))?;
    if (f.small.is_some() || f.pages.is_none()) && f.size.max(end) <= TINY_LIMIT as u64 {
        let mut bytes = f.small.take().unwrap_or_else(|| vec![0; f.size as usize]);
        bytes.resize(f.size.max(end) as usize, 0);
        bytes[offset as usize..end as usize].copy_from_slice(input);
        *f = FileObject::from_small(f.layout, bytes)?;
        return Ok(());
    }
    promote(s, f)?;
    let mut done = 0;
    while done < input.len() {
        let pos = offset + done as u64;
        let no = pos / f.layout.page_size as u64;
        let inside = (pos % f.layout.page_size as u64) as usize;
        let take = (input.len() - done).min(f.layout.page_size as usize - inside);
        let old = radix::get(s, f.pages, &no.to_be_bytes())?;
        let mut buffer = mutable_page(s, cache, f, no)?;
        buffer[inside..inside + take].copy_from_slice(&input[done..done + take]);
        let new = store_page(s, f.layout, &buffer)?;
        if old != new {
            f.pages = radix::set(s, f.pages, &no.to_be_bytes(), new)?;
        }
        done += take;
    }
    f.size = f.size.max(end);
    Ok(())
}
pub fn write_from<S: WriteStore + ?Sized, R: Read>(
    s: &S,
    cache: &ByteCache,
    mut file: Id,
    offset: u64,
    mut input: R,
) -> Result<(Id, u64)> {
    let mut f: FileObject = load(s, file, Kind::File)?;
    let mut buf = vec![0; f.layout.page_size as usize];
    let mut written = 0u64;
    loop {
        let n = match input.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        let pos = offset
            .checked_add(written)
            .ok_or_else(|| invalid("write offset overflow"))?;
        write_range_file(s, cache, &mut f, pos, &buf[..n])?;
        written = written
            .checked_add(n as u64)
            .ok_or_else(|| invalid("length overflow"))?;
    }
    if written == 0 {
        return Ok((file, 0));
    }
    file = save(s, Kind::File, &f)?;
    Ok((file, written))
}
pub fn append<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    input: &[u8],
) -> Result<Id> {
    if input.is_empty() {
        return Ok(file);
    }
    let mut f: FileObject = load(s, file, Kind::File)?;
    let offset = f.size;
    write_range_file(s, cache, &mut f, offset, input)?;
    save(s, Kind::File, &f)
}
pub fn truncate<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    size: u64,
) -> Result<Id> {
    let mut f: FileObject = load(s, file, Kind::File)?;
    if size == f.size {
        return Ok(file);
    }
    if size <= TINY_LIMIT as u64 && (f.small.is_some() || f.size > TINY_LIMIT as u64) {
        let mut bytes = vec![0; size as usize];
        read_into(s, cache, file, 0, &mut bytes)?;
        return save(s, Kind::File, &FileObject::from_small(f.layout, bytes)?);
    }
    promote(s, &mut f)?;
    if size >= f.size {
        f.size = size;
        return save(s, Kind::File, &f);
    }
    let p = f.layout.page_size as u64;
    let keep = size / p + u64::from(!size.is_multiple_of(p));
    f.pages = radix::remove_range(s, f.pages, &keep.to_be_bytes(), None)?;
    if !size.is_multiple_of(p) {
        let no = size / p;
        if radix::get(s, f.pages, &no.to_be_bytes())?.is_some() {
            let mut buffer = mutable_page(s, cache, &f, no)?;
            buffer[(size % p) as usize..].fill(0);
            let new = store_page(s, f.layout, &buffer)?;
            f.pages = radix::set(s, f.pages, &no.to_be_bytes(), new)?;
        }
    }
    f.size = size;
    save(s, Kind::File, &f)
}
pub fn punch_hole<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    file: Id,
    offset: u64,
    length: u64,
) -> Result<Id> {
    let mut f: FileObject = load(s, file, Kind::File)?;
    if offset >= f.size || length == 0 {
        return Ok(file);
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("hole offset overflow"))?
        .min(f.size);
    if let Some(mut bytes) = f.small.take() {
        bytes[offset as usize..end as usize].fill(0);
        return save(s, Kind::File, &FileObject::from_small(f.layout, bytes)?);
    }
    let p = f.layout.page_size as u64;
    let first = offset / p;
    let last = (end - 1) / p;
    // At most two partial pages; no iteration through an enormous sparse hole.
    for no in [first, last] {
        if no == first && no == last && offset.is_multiple_of(p) && end % p == 0 {
            continue;
        }
        let page_start = no * p;
        let a = offset.saturating_sub(page_start).min(p) as usize;
        let b = (end - page_start).min(p) as usize;
        if a == 0 && b == p as usize {
            continue;
        }
        if radix::get(s, f.pages, &no.to_be_bytes())?.is_none() {
            continue;
        }
        let mut buf = mutable_page(s, cache, &f, no)?;
        buf[a..b].fill(0);
        let new = store_page(s, f.layout, &buf)?;
        f.pages = radix::set(s, f.pages, &no.to_be_bytes(), new)?;
        if first == last {
            break;
        }
    }
    let begin = offset / p + u64::from(!offset.is_multiple_of(p));
    let stop = end / p;
    if begin < stop {
        f.pages = radix::remove_range(s, f.pages, &begin.to_be_bytes(), Some(&stop.to_be_bytes()))?;
    }
    save(s, Kind::File, &f)
}
/// Reflink-like copy, not a hard link. Aligned ranges reuse page manifests;
/// unaligned ranges stream through one page-sized buffer. Source is immutable,
/// so overlap has memmove-like snapshot semantics.
pub fn clone_range<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    source: Id,
    source_offset: u64,
    dest: Id,
    dest_offset: u64,
    length: u64,
) -> Result<Id> {
    let a: FileObject = load(s, source, Kind::File)?;
    let mut b: FileObject = load(s, dest, Kind::File)?;
    let a_end = source_offset
        .checked_add(length)
        .ok_or_else(|| invalid("source range overflow"))?;
    let b_end = dest_offset
        .checked_add(length)
        .ok_or_else(|| invalid("destination range overflow"))?;
    if a_end > a.size {
        return Err(invalid("clone range exceeds source EOF"));
    }
    if length == 0 {
        return Ok(dest);
    }
    let p = a.layout.page_size as u64;
    if a.small.is_none()
        && b.small.is_none()
        && a.layout == b.layout
        && source_offset.is_multiple_of(p)
        && dest_offset.is_multiple_of(p)
        && length.is_multiple_of(p)
    {
        let src = source_offset / p;
        let dst = dest_offset / p;
        let count = length / p;
        b.pages = radix::remove_range(
            s,
            b.pages,
            &dst.to_be_bytes(),
            Some(&(dst + count).to_be_bytes()),
        )?;
        let mut root = b.pages;
        radix::visit(s, a.pages, &mut |key, id| {
            let key: [u8; 8] = key
                .try_into()
                .map_err(|_| corrupt("invalid page-map key"))?;
            let no = u64::from_be_bytes(key);
            if no >= src && no - src < count {
                root = radix::set(s, root, &(dst + no - src).to_be_bytes(), Some(id))?;
            }
            Ok(())
        })?;
        b.pages = root;
        b.size = b.size.max(b_end);
        return save(s, Kind::File, &b);
    }
    let mut output = dest;
    let mut done = 0u64;
    let mut buffer = vec![0; a.layout.page_size as usize];
    while done < length {
        let take = (length - done).min(buffer.len() as u64) as usize;
        let n = read_into(s, cache, source, source_offset + done, &mut buffer[..take])?;
        if n != take {
            return Err(corrupt("short clone source"));
        }
        output = write_range(s, cache, output, dest_offset + done, &buffer[..n])?;
        done += n as u64;
    }
    Ok(output)
}
pub struct FileReader<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    cache: &'a ByteCache,
    file: Id,
    position: u64,
    size: u64,
}
impl<'a, S: ReadStore + ?Sized> FileReader<'a, S> {
    pub fn new(store: &'a S, cache: &'a ByteCache, file: Id) -> Result<Self> {
        let f: FileObject = load(store, file, Kind::File)?;
        Ok(Self {
            store,
            cache,
            file,
            position: 0,
            size: f.size,
        })
    }
}
impl<S: ReadStore + ?Sized> Read for FileReader<'_, S> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = read_into(self.store, self.cache, self.file, self.position, b)
            .map_err(std::io::Error::other)?;
        self.position += n as u64;
        Ok(n)
    }
}
impl<S: ReadStore + ?Sized> Seek for FileReader<'_, S> {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        let next = match p {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.position as i128 + n as i128,
            SeekFrom::End(n) => self.size as i128 + n as i128,
        };
        if !(0..=u64::MAX as i128).contains(&next) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek overflow",
            ));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}
