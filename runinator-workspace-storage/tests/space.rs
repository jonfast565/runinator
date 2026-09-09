//! Regression coverage for the four v0.8 physical-space changes.
use runinator_workspace_storage::{
    Id, Layout, Result,
    cache::ByteCache,
    codec::{Binary, Encoder},
    index::{self, DiskIndex, IndexWriter, Location, STANDALONE},
    model::{ChunkRef, FileObject, INLINE_LIMIT, Kind, Page, PageExtent, TINY_LIMIT, ZERO_RUN_MIN},
    pages, record,
    store::{MemoryStore, load},
    tiny,
};
use std::io::{Read, Seek, SeekFrom, Write};

fn data(n: usize) -> Vec<u8> {
    let mut state = 0x243f6a8885a308d3u64;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}
fn small_layout() -> Layout {
    Layout {
        page_size: 65536,
        min: 256,
        avg: 1024,
        max: 4096,
    }
}

#[test]
fn inline_file_is_one_leaf_without_page_objects() -> Result<()> {
    let s = MemoryStore::default();
    let cache = ByteCache::new(1024);
    let file = pages::ingest(&s, Layout::default(), b"key=value".as_slice())?;
    assert_eq!(s.len(), 1);
    let f: FileObject = load(&s, file, Kind::File)?;
    assert_eq!(f.small.as_deref(), Some(b"key=value".as_slice()));
    assert!(f.pages.is_none());
    assert!(!f.is_tiny());
    assert_eq!(pages::read_range(&s, &cache, file, 0, 100)?, b"key=value");
    assert_eq!(cache.stats()?.resident_bytes, 0);
    assert_eq!(
        pages::ingest(&s, Layout::default(), b"key=value".as_slice())?,
        file
    );
    assert_eq!(s.len(), 1);
    Ok(())
}

#[test]
fn inline_tiny_and_paged_thresholds_are_exact() -> Result<()> {
    for n in [
        0,
        1,
        INLINE_LIMIT,
        INLINE_LIMIT + 1,
        TINY_LIMIT,
        TINY_LIMIT + 1,
    ] {
        let s = MemoryStore::default();
        let bytes = data(n);
        let id = pages::ingest(&s, small_layout(), bytes.as_slice())?;
        let f: FileObject = load(&s, id, Kind::File)?;
        assert_eq!(f.size, n as u64);
        assert_eq!(f.small.is_some(), n <= TINY_LIMIT);
        assert_eq!(f.is_tiny(), n > INLINE_LIMIT && n <= TINY_LIMIT);
        assert_eq!(
            pages::read_range(&s, &ByteCache::new(2 * 65536), id, 0, n)?,
            bytes
        );
    }
    Ok(())
}

#[test]
fn small_leaf_promotes_and_shrinks_without_resurrecting_bytes() -> Result<()> {
    let s = MemoryStore::default();
    let cache = ByteCache::new(2 * 65536);
    let old_bytes = data(60000);
    let old = pages::ingest(&s, small_layout(), old_bytes.as_slice())?;
    let grown = pages::write_range(&s, &cache, old, 70000, b"xyz")?;
    assert!(
        load::<FileObject, _>(&s, grown, Kind::File)?
            .small
            .is_none()
    );
    assert_eq!(
        pages::read_range(&s, &cache, grown, 69997, 6)?,
        b"\0\0\0xyz"
    );
    let short = pages::truncate(&s, &cache, grown, 20)?;
    assert!(
        load::<FileObject, _>(&s, short, Kind::File)?
            .small
            .is_some()
    );
    let extended = pages::truncate(&s, &cache, short, 200)?;
    let bytes = pages::read_range(&s, &cache, extended, 0, 200)?;
    assert_eq!(&bytes[..20], &old_bytes[..20]);
    assert!(bytes[20..].iter().all(|b| *b == 0));
    assert_eq!(
        pages::read_range(&s, &cache, old, 0, old_bytes.len())?,
        old_bytes
    );
    Ok(())
}

#[test]
fn small_noop_write_and_hole_punch_preserve_semantics() -> Result<()> {
    let s = MemoryStore::default();
    let cache = ByteCache::new(65536);
    let id = pages::ingest(&s, small_layout(), b"abcdefgh".as_slice())?;
    let count = s.len();
    assert_eq!(pages::write_range(&s, &cache, id, 2, b"cd")?, id);
    assert_eq!(s.len(), count);
    let hole = pages::punch_hole(&s, &cache, id, 2, 3)?;
    assert_eq!(pages::read_range(&s, &cache, hole, 0, 8)?, b"ab\0\0\0fgh");
    let zero = pages::punch_hole(&s, &cache, hole, 0, 8)?;
    let f: FileObject = load(&s, zero, Kind::File)?;
    assert!(f.small.is_none() && f.pages.is_none());
    assert_eq!(f.size, 8);
    Ok(())
}

struct ShortReads {
    bytes: std::io::Cursor<Vec<u8>>,
    interrupted: bool,
}
impl Read for ShortReads {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = out.len().min(17);
        self.bytes.read(&mut out[..n])
    }
}
#[test]
fn bounded_lookahead_handles_short_reads_and_interruption() -> Result<()> {
    let s = MemoryStore::default();
    let bytes = data(TINY_LIMIT + 1);
    let input = ShortReads {
        bytes: std::io::Cursor::new(bytes.clone()),
        interrupted: false,
    };
    let id = pages::ingest(&s, small_layout(), input)?;
    assert_eq!(
        pages::read_range(&s, &ByteCache::new(2 * 65536), id, 0, bytes.len())?,
        bytes
    );
    Ok(())
}

#[test]
fn page_policy_selects_one_four_and_eight_mib() {
    const M: u64 = 1024 * 1024;
    assert_eq!(Layout::default().page_size, 4 * M as u32);
    assert_eq!(Layout::for_size(16 * M - 1).page_size, M as u32);
    assert_eq!(Layout::for_size(16 * M).page_size, 4 * M as u32);
    assert_eq!(Layout::for_size(1024 * M - 1).page_size, 4 * M as u32);
    assert_eq!(Layout::for_size(1024 * M).page_size, 8 * M as u32);
}

#[test]
fn growth_does_not_silently_repage_existing_files() -> Result<()> {
    let s = MemoryStore::default();
    let layout = Layout::default();
    let cache = ByteCache::new(2 * layout.page_size as usize);
    let file = pages::ingest(&s, layout, std::io::repeat(3).take(5 * 1024 * 1024))?;
    let p0 = pages::page_id(&s, file, 0)?;
    let edited = pages::write_range(&s, &cache, file, layout.page_size as u64 + 20, b"edit")?;
    assert_eq!(pages::page_id(&s, edited, 0)?, p0);
    let grown = pages::truncate(&s, &cache, edited, 2 * 1024 * 1024 * 1024)?;
    assert_eq!(load::<FileObject, _>(&s, grown, Kind::File)?.layout, layout);
    assert_eq!(pages::page_id(&s, grown, 0)?, p0);
    Ok(())
}

#[test]
fn interior_zero_runs_have_no_chunk_objects() -> Result<()> {
    let s = MemoryStore::default();
    let layout = small_layout();
    let mut input = vec![1; 2000];
    input.extend(vec![0; 20000]);
    input.extend(vec![2; 4000]);
    input.extend(vec![0; 9000]);
    input.extend(vec![7; 100]);
    let page_id = pages::store_page(&s, layout, &input)?.unwrap();
    let p: Page = load(&s, page_id, Kind::Page)?;
    let zeros: Vec<_> = p
        .extents
        .iter()
        .filter_map(|e| match e {
            PageExtent::Zero(n) => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(zeros, vec![20000, 9000]);
    let data_bytes: u32 = p
        .extents
        .iter()
        .filter_map(|e| match e {
            PageExtent::Data(c) => Some(c.len),
            _ => None,
        })
        .sum();
    assert_eq!(data_bytes, 6100);
    let decoded = pages::pin_page(&s, &ByteCache::new(65536), page_id, layout.page_size)?;
    assert_eq!(&decoded[..input.len()], input.as_slice());
    assert!(decoded[input.len()..].iter().all(|b| *b == 0));
    Ok(())
}

#[test]
fn zero_run_threshold_and_implicit_tail_are_canonical() -> Result<()> {
    for len in [ZERO_RUN_MIN - 1, ZERO_RUN_MIN, ZERO_RUN_MIN + 1] {
        let s = MemoryStore::default();
        let mut bytes = vec![1];
        bytes.extend(vec![0; len]);
        bytes.push(2);
        bytes.extend(vec![0; 200]);
        let id = pages::store_page(&s, small_layout(), &bytes)?.unwrap();
        let p: Page = load(&s, id, Kind::Page)?;
        assert_eq!(p.used as usize, len + 2);
        assert_eq!(
            p.extents.iter().any(|e| matches!(e, PageExtent::Zero(_))),
            len >= ZERO_RUN_MIN
        );
    }
    assert!(pages::store_page(&MemoryStore::default(), small_layout(), &[0; 65536])?.is_none());
    Ok(())
}

#[test]
fn malformed_zero_extents_and_small_file_tags_are_rejected() -> Result<()> {
    let c = ChunkRef {
        id: Id::default(),
        len: 1,
    };
    for extents in [
        vec![PageExtent::Zero(1), PageExtent::Data(c.clone())],
        vec![PageExtent::Data(c.clone()), PageExtent::Zero(4096)],
        vec![
            PageExtent::Zero(4096),
            PageExtent::Zero(4096),
            PageExtent::Data(c.clone()),
        ],
    ] {
        let used = extents.iter().map(PageExtent::len).sum();
        assert!(
            Page {
                page_size: 65536,
                used,
                extents
            }
            .encode()
            .is_err()
        );
    }
    let mut encoded = FileObject::from_small(small_layout(), vec![1; 129])?.encode()?;
    // Version + size + four layout integers + normalization + seed = 34 bytes.
    encoded[34] = 1;
    assert!(FileObject::decode(&encoded).is_err());
    let mut e = Encoder::new();
    e.u32(65536);
    e.u32(5000);
    e.u32(1);
    e.u8(0);
    e.u32(5000);
    assert!(Page::decode(&e.finish()?).is_err());
    Ok(())
}

fn location(id: Id, pack: Id, slot: u32) -> Location {
    Location {
        id,
        pack,
        offset: 8,
        length: 100,
        member: slot,
    }
}
#[test]
fn compact_index_is_52_bytes_per_object_plus_one_pack_dictionary() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut writer = IndexWriter::new(dir.path())?;
    let packs = [Id::sha256(b"pack-a"), Id::sha256(b"pack-b")];
    let mut ids: Vec<_> = (0u64..100).map(|n| Id::sha256(&n.to_le_bytes())).collect();
    ids.sort();
    for (n, id) in ids.iter().enumerate() {
        writer.push(location(*id, packs[n % 2], STANDALONE))?;
    }
    let file = writer.finish()?;
    assert_eq!(file.as_file().metadata()?.len(), 24 + 2 * 32 + 100 * 52);
    assert!(file.as_file().metadata()?.len() < 16 + 100 * 80);
    let index = DiskIndex::open(file.path())?;
    index.validate()?;
    assert_eq!(index.pack_count(), 2);
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(index.lookup(*id)?.unwrap().pack, packs[n % 2]);
    }
    Ok(())
}

#[test]
fn compact_index_merge_remaps_pack_slots_and_keeps_old_duplicates() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let id = Id::sha256(b"same");
    let old_pack = Id::sha256(b"old");
    let new_pack = Id::sha256(b"new");
    let mut a = IndexWriter::new(dir.path())?;
    a.push(location(id, old_pack, 7))?;
    let a = a.finish()?;
    let mut b = IndexWriter::new(dir.path())?;
    let mut entries = vec![
        location(id, new_pack, 99),
        location(Id::sha256(b"other"), new_pack, 8),
    ];
    entries.sort_by_key(|e| e.id);
    for e in entries {
        b.push(e)?;
    }
    let b = b.finish()?;
    let merged = index::merge(
        &DiskIndex::open(a.path())?,
        &DiskIndex::open(b.path())?,
        dir.path(),
    )?;
    let index = DiskIndex::open(merged.path())?;
    let entry = index.lookup(id)?.unwrap();
    assert_eq!((entry.pack, entry.member), (old_pack, 7));
    let entry = index.lookup(Id::sha256(b"other"))?.unwrap();
    assert_eq!((entry.pack, entry.member), (new_pack, 8));
    Ok(())
}

#[test]
fn compact_index_rejects_invalid_pack_slot_and_truncated_table() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let id = Id::sha256(b"entry");
    let mut writer = IndexWriter::new(dir.path())?;
    writer.push(location(id, Id::sha256(b"pack"), STANDALONE))?;
    let mut file = writer.finish()?;
    file.seek(SeekFrom::Start(24 + 32 + 32))?;
    file.write_all(&u32::MAX.to_le_bytes())?;
    let index = DiskIndex::open(file.path())?;
    assert!(index.lookup(id).is_err());
    file.as_file().set_len(24 + 31)?;
    assert!(DiskIndex::open(file.path()).is_err());
    Ok(())
}

fn tiny_payload(byte: u8) -> Result<(Id, Vec<u8>)> {
    let raw = FileObject::from_small(small_layout(), vec![byte; 600])?.encode()?;
    Ok((Id::object(Kind::File, &raw), raw))
}
#[test]
fn tiny_block_preserves_leaf_ids_and_expands_on_pack_import() -> Result<()> {
    let mut pending = tiny::Pending::default();
    let mut expected = vec![tiny_payload(1)?, tiny_payload(2)?];
    for (id, raw) in &expected {
        pending.push(*id, raw)?;
    }
    let (raw, ids) = pending.take()?.unwrap();
    expected.sort_by_key(|e| e.0);
    assert_eq!(ids, expected.iter().map(|e| e.0).collect::<Vec<_>>());
    for (slot, (id, bytes)) in expected.iter().enumerate() {
        assert_eq!(tiny::member(&raw, slot as u32, *id)?.raw, bytes.as_slice());
    }
    let mut pack = tempfile::NamedTempFile::new()?;
    pack.write_all(record::PACK_MAGIC)?;
    record::write(&mut pack, Kind::TinyBlock, &raw)?;
    let mut imported = Vec::new();
    assert_eq!(
        record::visit_pack(pack.path(), |id, obj| {
            assert_eq!(obj.kind, Kind::File);
            imported.push((id, (*obj.bytes).clone()));
            Ok(())
        })?,
        2
    );
    assert_eq!(imported, expected);
    let cache = ByteCache::new(1024 * 1024);
    let length = pack.as_file().metadata()?.len() - 8;
    for (slot, (id, bytes)) in expected.iter().enumerate() {
        let location = Location {
            id: *id,
            pack: Id::default(),
            offset: 8,
            length,
            member: slot as u32,
        };
        assert_eq!(
            record::read_indexed(pack.as_file(), location, &cache)?
                .bytes
                .as_slice(),
            bytes
        );
        assert!(
            record::read_indexed(
                pack.as_file(),
                Location {
                    id: Id::default(),
                    ..location
                },
                &cache
            )
            .is_err()
        );
        assert!(
            record::read_indexed(
                pack.as_file(),
                Location {
                    length: length - 1,
                    ..location
                },
                &cache
            )
            .is_err()
        );
    }
    assert_eq!(cache.stats()?.misses, 1);
    assert!(cache.stats()?.hits >= 1);
    assert!(pack.as_file().metadata()?.len() < 8 + 2 * record::HEADER_LEN + raw.len() as u64);
    Ok(())
}

#[test]
fn tiny_blocks_reject_bad_slots_hashes_sizes_and_duplicates() -> Result<()> {
    let (id, bytes) = tiny_payload(3)?;
    let mut pending = tiny::Pending::default();
    pending.push(id, &bytes)?;
    let (mut raw, _) = pending.take()?.unwrap();
    assert!(tiny::member(&raw, 1, id).is_err());
    assert!(tiny::member(&raw, 0, Id::default()).is_err());
    let last = raw.len() - 1;
    raw[last] ^= 1;
    assert!(tiny::member(&raw, 0, id).is_err());
    assert!(tiny::table(&vec![0; tiny::BLOCK_LIMIT + 1]).is_err());
    pending.push(id, &bytes)?;
    pending.push(id, &bytes)?;
    assert!(pending.take().is_err());
    Ok(())
}

#[test]
fn tiny_block_accumulator_never_exceeds_256_kib() -> Result<()> {
    let mut pending = tiny::Pending::default();
    let mut blocks = 0;
    for i in 1u8..8 {
        let raw = FileObject::from_small(small_layout(), vec![i; TINY_LIMIT])?.encode()?;
        let id = Id::object(Kind::File, &raw);
        if pending.would_overflow(raw.len()) {
            let (bytes, _) = pending.take()?.unwrap();
            assert!(bytes.len() <= tiny::BLOCK_LIMIT);
            blocks += 1;
        }
        pending.push(id, &raw)?;
    }
    if let Some((bytes, _)) = pending.take()? {
        assert!(bytes.len() <= tiny::BLOCK_LIMIT);
        blocks += 1;
    }
    assert!(blocks >= 3);
    Ok(())
}
