use proptest::prelude::*;
use runinator_workspace_storage::{
    Error, Id, Layout, Result,
    cache::ByteCache,
    codec::Binary,
    model::{FileObject, Kind, Page, PageExtent, RadixNode},
    pages, radix,
    store::{MemoryStore, WriteStore, load, save},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
fn layout() -> Layout {
    Layout {
        page_size: 4096,
        min: 64,
        avg: 256,
        max: 1024,
    }
}
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
fn cache() -> ByteCache {
    ByteCache::new(4 * 4096)
}
#[test]
fn object_identity_is_type_separated() {
    assert_ne!(Id::object(Kind::Chunk, b"x"), Id::object(Kind::File, b"x"));
}
#[test]
fn digest_text_is_strict() {
    let id = Id::sha256(b"test");
    assert_eq!(id.to_string().parse::<Id>().unwrap(), id);
    assert!("../bad".parse::<Id>().is_err());
    assert!("A".repeat(64).parse::<Id>().is_err());
}
#[test]
fn metadata_rejects_trailing_bytes() -> Result<()> {
    let f = FileObject {
        size: 0,
        layout: layout(),
        pages: None,
        small: None,
    };
    let mut b = f.encode()?;
    b.push(0);
    assert!(FileObject::decode(&b).is_err());
    Ok(())
}
#[test]
fn layout_rejects_library_panic_parameters() {
    let mut l = layout();
    l.avg = 64;
    assert!(l.validate().is_err());
    l = layout();
    l.max = 512;
    assert!(l.validate().is_err());
    l = layout();
    l.page_size = 5000;
    assert!(l.validate().is_err());
}
#[test]
fn radix_order_independence_and_pruning() -> Result<()> {
    let store = MemoryStore::default();
    let value = store.put(Kind::Chunk, b"value")?;
    let keys = [b"alpha".as_slice(), b"alpine", b"beta", b"", b"alphabet"];
    let mut a = None;
    for k in keys {
        a = radix::set(&store, a, k, Some(value))?;
    }
    let mut b = None;
    for k in keys.into_iter().rev() {
        b = radix::set(&store, b, k, Some(value))?;
    }
    assert_eq!(a, b);
    for k in keys {
        assert_eq!(radix::get(&store, a, k)?, Some(value));
        a = radix::set(&store, a, k, None)?;
    }
    assert_eq!(a, None);
    Ok(())
}
#[test]
fn radix_noop_has_no_new_objects() -> Result<()> {
    let s = MemoryStore::default();
    let v = s.put(Kind::Chunk, b"v")?;
    let root = radix::set(&s, None, b"abc", Some(v))?;
    let n = s.len();
    assert_eq!(radix::set(&s, root, b"abc", Some(v))?, root);
    assert_eq!(radix::set(&s, root, b"absent", None)?, root);
    assert_eq!(s.len(), n);
    Ok(())
}
#[test]
fn radix_range_removal_preserves_outside_values() -> Result<()> {
    let s = MemoryStore::default();
    let v = s.put(Kind::Chunk, b"v")?;
    let keys = [0u64, 1, 2, 255, 256, 257, 1000, u64::MAX];
    let mut root = None;
    for k in keys {
        root = radix::set(&s, root, &k.to_be_bytes(), Some(v))?;
    }
    root = radix::remove_range(&s, root, &2u64.to_be_bytes(), Some(&257u64.to_be_bytes()))?;
    for k in keys {
        assert_eq!(
            radix::get(&s, root, &k.to_be_bytes())?.is_some(),
            !(2..257).contains(&k)
        );
    }
    root = radix::remove_range(&s, root, &1000u64.to_be_bytes(), None)?;
    assert_eq!(radix::get(&s, root, &u64::MAX.to_be_bytes())?, None);
    Ok(())
}
#[test]
fn malformed_radix_node_is_rejected() -> Result<()> {
    let s = MemoryStore::default();
    let id = save(&s, Kind::Radix, &RadixNode::default())?;
    assert!(load::<RadixNode, _>(&s, id, Kind::Radix).is_err());
    Ok(())
}
#[test]
fn typed_load_rejects_wrong_kind() -> Result<()> {
    let s = MemoryStore::default();
    let id = s.put(Kind::Chunk, b"abc")?;
    assert!(load::<FileObject, _>(&s, id, Kind::File).is_err());
    Ok(())
}
struct ShortReader {
    bytes: Vec<u8>,
    pos: usize,
    largest: usize,
}
impl Read for ShortReader {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.largest = self.largest.max(b.len());
        let n = (self.bytes.len() - self.pos).min(b.len()).min(17);
        b[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
#[test]
fn ingestion_tolerates_short_reads_and_is_page_bounded() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let input = data(3 * 4096 + 91);
    let mut reader = ShortReader {
        bytes: input.clone(),
        pos: 0,
        largest: 0,
    };
    let id = pages::ingest_paged(&s, layout(), &mut reader)?;
    assert!(reader.largest <= 4096);
    assert_eq!(pages::read_range(&s, &c, id, 0, input.len() + 100)?, input);
    Ok(())
}
#[test]
fn cow_write_reuses_neighboring_pages() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let input = data(3 * 4096);
    let old = pages::ingest_paged(&s, layout(), input.as_slice())?;
    let new = pages::write_range(&s, &c, old, 4096 + 10, b"changed")?;
    assert_eq!(pages::page_id(&s, old, 0)?, pages::page_id(&s, new, 0)?);
    assert_eq!(pages::page_id(&s, old, 2)?, pages::page_id(&s, new, 2)?);
    assert_ne!(pages::page_id(&s, old, 1)?, pages::page_id(&s, new, 1)?);
    assert_eq!(pages::read_range(&s, &c, old, 0, input.len())?, input);
    let mut expected = input;
    expected[4106..4113].copy_from_slice(b"changed");
    assert_eq!(pages::read_range(&s, &c, new, 0, expected.len())?, expected);
    Ok(())
}
#[test]
fn noop_write_preserves_file_identity() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let bytes = data(8192);
    let old = pages::ingest_paged(&s, layout(), bytes.as_slice())?;
    let count = s.len();
    let new = pages::write_range(&s, &c, old, 100, &bytes[100..200])?;
    assert_eq!(old, new);
    assert_eq!(s.len(), count);
    Ok(())
}
#[test]
fn fastcdc_reuses_chunks_inside_a_modified_page() -> Result<()> {
    let s = MemoryStore::default();
    let c = ByteCache::new(2 * 65536);
    let l = Layout {
        page_size: 65536,
        min: 256,
        avg: 1024,
        max: 4096,
    };
    let input = data(65536);
    let old = pages::ingest_paged(&s, l, input.as_slice())?;
    let new = pages::write_range(&s, &c, old, 32000, &[input[32000] ^ 255])?;
    let a: Page = load(&s, pages::page_id(&s, old, 0)?.unwrap(), Kind::Page)?;
    let b: Page = load(&s, pages::page_id(&s, new, 0)?.unwrap(), Kind::Page)?;
    let ids = |p: &Page| {
        p.extents
            .iter()
            .filter_map(|e| match e {
                PageExtent::Data(c) => Some(c.id),
                PageExtent::Zero(_) => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(ids(&a).iter().any(|id| ids(&b).contains(id)));
    assert_ne!(old, new);
    Ok(())
}
#[test]
fn zero_data_is_sparse_and_tail_padding_is_not_stored() -> Result<()> {
    let s = MemoryStore::default();
    let empty = pages::ingest_paged(&s, layout(), vec![0; 10000].as_slice())?;
    let f: FileObject = load(&s, empty, Kind::File)?;
    assert_eq!(f.pages, None);
    assert_eq!(s.len(), 1);
    let id = pages::store_page(&s, layout(), &[7, 0, 0, 0])?.unwrap();
    let p: Page = load(&s, id, Kind::Page)?;
    assert_eq!(p.used, 1);
    Ok(())
}
#[test]
fn sparse_far_write_allocates_no_intermediate_pages() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::empty_file(&s, layout())?;
    let offset = 4096u64 * 1_000_000_000 + 19;
    let id = pages::write_range(&s, &c, old, offset, b"hello")?;
    assert!(s.len() < 30);
    assert_eq!(
        pages::read_range(&s, &c, id, offset - 4, 9)?,
        b"\0\0\0\0hello"
    );
    Ok(())
}
#[test]
fn cross_page_write_and_overflow_checks() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::ingest_paged(&s, layout(), vec![1; 8192].as_slice())?;
    let new = pages::write_range(&s, &c, old, 4090, &[2; 32])?;
    assert_eq!(pages::read_range(&s, &c, new, 4090, 32)?, vec![2; 32]);
    assert!(pages::write_range(&s, &c, new, u64::MAX, b"xx").is_err());
    assert_eq!(pages::write_range(&s, &c, new, u64::MAX, b"")?, new);
    Ok(())
}
#[test]
fn truncate_then_extend_never_resurrects_old_tail() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::ingest_paged(&s, layout(), vec![9; 10000].as_slice())?;
    let short = pages::truncate(&s, &c, old, 4100)?;
    let grown = pages::truncate(&s, &c, short, 10000)?;
    let bytes = pages::read_range(&s, &c, grown, 0, 10000)?;
    assert_eq!(&bytes[..4100], vec![9; 4100]);
    assert!(bytes[4100..].iter().all(|&b| b == 0));
    assert_eq!(pages::page_id(&s, grown, 2)?, None);
    Ok(())
}
#[test]
fn truncate_zero_then_huge_extension_stays_sparse() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let file = pages::ingest_paged(&s, layout(), b"secret".as_slice())?;
    let file = pages::truncate(&s, &c, file, 0)?;
    let count = s.len();
    let file = pages::truncate(&s, &c, file, 1 << 50)?;
    let f: FileObject = load(&s, file, Kind::File)?;
    assert_eq!(f.pages, None);
    assert_eq!(s.len(), count + 1);
    Ok(())
}
#[test]
fn hole_punch_handles_partial_full_and_sparse_pages() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::ingest_paged(&s, layout(), vec![5; 4 * 4096].as_slice())?;
    let new = pages::punch_hole(&s, &c, old, 100, 12000)?;
    let bytes = pages::read_range(&s, &c, new, 0, 4 * 4096)?;
    assert!(bytes[..100].iter().all(|&b| b == 5));
    assert!(bytes[100..12100].iter().all(|&b| b == 0));
    assert!(bytes[12100..].iter().all(|&b| b == 5));
    assert_eq!(pages::page_id(&s, new, 1)?, None);
    let zero = pages::punch_hole(&s, &c, new, 0, 4 * 4096)?;
    assert_eq!(load::<FileObject, _>(&s, zero, Kind::File)?.pages, None);
    Ok(())
}
#[test]
fn single_page_hole_preserves_surrounding_bytes() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::ingest_paged(&s, layout(), b"abcdefgh".as_slice())?;
    let new = pages::punch_hole(&s, &c, old, 2, 3)?;
    assert_eq!(pages::read_range(&s, &c, new, 0, 8)?, b"ab\0\0\0fgh");
    Ok(())
}
#[test]
fn aligned_clone_shares_pages() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let src = pages::ingest_paged(&s, layout(), data(8192).as_slice())?;
    let dst = pages::ingest_paged(&s, layout(), std::io::empty())?;
    let new = pages::clone_range(&s, &c, src, 0, dst, 4096, 8192)?;
    assert_eq!(pages::page_id(&s, src, 0)?, pages::page_id(&s, new, 1)?);
    assert_eq!(pages::page_id(&s, src, 1)?, pages::page_id(&s, new, 2)?);
    assert_eq!(pages::page_id(&s, new, 0)?, None);
    Ok(())
}
#[test]
fn overlapping_unaligned_clone_reads_immutable_source() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let old = pages::ingest_paged(&s, layout(), b"0123456789".as_slice())?;
    let new = pages::clone_range(&s, &c, old, 0, old, 2, 8)?;
    assert_eq!(pages::read_range(&s, &c, new, 0, 10)?, b"0101234567");
    Ok(())
}
#[test]
fn seekable_reader_handles_eof_and_invalid_seek() -> Result<()> {
    let s = MemoryStore::default();
    let c = cache();
    let f = pages::ingest_paged(&s, layout(), b"abcdef".as_slice())?;
    let mut r = pages::FileReader::new(&s, &c, f)?;
    r.seek(SeekFrom::End(-2))?;
    let mut out = Vec::new();
    r.read_to_end(&mut out)?;
    assert_eq!(out, b"ef");
    assert!(r.seek(SeekFrom::Current(-100)).is_err());
    Ok(())
}
#[test]
fn cache_pins_enforce_budget_and_drop_unpins() -> Result<()> {
    let c = ByteCache::new(4);
    let a = Id::sha256(b"a");
    let b = Id::sha256(b"b");
    let pin = c.get_or_load(a, 4, || Ok(vec![1; 4]))?;
    assert!(matches!(
        c.get_or_load(b, 4, || Ok(vec![2; 4])),
        Err(Error::CacheFull)
    ));
    drop(pin);
    assert_eq!(&*c.get_or_load(b, 4, || Ok(vec![2; 4]))?, &vec![2; 4]);
    assert!(c.stats()?.resident_bytes <= 4);
    Ok(())
}
#[test]
fn failed_cache_load_releases_reservation() -> Result<()> {
    let c = ByteCache::new(4);
    let id = Id::sha256(b"x");
    assert!(
        c.get_or_load(id, 4, || Err(Error::Invalid("failure".into())))
            .is_err()
    );
    assert_eq!(c.stats()?.reserved_bytes, 0);
    c.get_or_load(id, 4, || Ok(vec![0; 4]))?;
    Ok(())
}
#[test]
fn concurrent_cache_misses_are_coalesced() -> Result<()> {
    let cache = Arc::new(ByteCache::new(32));
    let barrier = Arc::new(Barrier::new(2));
    let loads = Arc::new(AtomicUsize::new(0));
    let id = Id::sha256(b"x");
    let mut threads = Vec::new();
    for _ in 0..2 {
        let c = cache.clone();
        let b = barrier.clone();
        let n = loads.clone();
        threads.push(std::thread::spawn(move || {
            b.wait();
            c.get_or_load(id, 32, || {
                n.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                Ok(vec![3; 32])
            })
        }));
    }
    for t in threads {
        assert_eq!(t.join().unwrap()?.len(), 32);
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    Ok(())
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn randomized_radix_matches_ordered_map(ops in prop::collection::vec((0u64..2000, any::<bool>()),0..80)) {
        let s=MemoryStore::default();
        let value=s.put(Kind::Chunk,b"x").unwrap();
        let mut root=None;
        let mut expected=BTreeMap::new();
        for (key,insert) in ops {
            root=radix::set(&s,root,&key.to_be_bytes(),if insert {
                Some(value)
            } else {
                None
            }).unwrap();
            if insert {
                expected.insert(key,value);
            } else {
                expected.remove(&key);
            }
        }
        let mut actual=BTreeMap::new();
        radix::visit(&s,root,&mut |k,id| {
            actual.insert(u64::from_be_bytes(k.try_into().unwrap()),id);
            Ok(())
        }).unwrap();
        prop_assert_eq!(actual,expected);
    }
    #[test]
    fn randomized_file_edits_match_a_byte_vector(ops in prop::collection::vec((0u8..3,0usize..20000,prop::collection::vec(any::<u8>(),0..64)),0..20)) {
        let s=MemoryStore::default();
        let c=cache();
        let mut id=pages::empty_file(&s,layout()).unwrap();
        let mut expected=Vec::new();
        for (kind,offset,bytes) in ops {
            match kind {
                0=> {
                    id=pages::write_range(&s,&c,id,offset as u64,&bytes).unwrap();
                    if !bytes.is_empty() {
                        if expected.len()<offset+bytes.len() {
                            expected.resize(offset+bytes.len(),0);
                        }
                        expected[offset..offset+bytes.len()].copy_from_slice(&bytes);
                    }
                },
                1=> {
                    id=pages::truncate(&s,&c,id,offset as u64).unwrap();
                    expected.resize(offset,0);
                },
                _=> {
                    id=pages::punch_hole(&s,&c,id,offset as u64,bytes.len() as u64).unwrap();
                    let end=(offset+bytes.len()).min(expected.len());
                    if offset<end {
                        expected[offset..end].fill(0);
                    }
                }
            }
            prop_assert_eq!(pages::read_range(&s,&c,id,0,25000).unwrap(),expected.clone());
        }
    }
}
