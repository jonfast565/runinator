use runinator_workspace_storage::{
    Id, Result,
    codec::{Binary, Encoder},
    index::{DiskIndex, ExternalSorter, IndexWriter, Location},
    model::{Kind, Page},
    record,
};
use std::io::{Seek, SeekFrom, Write};
#[test]
fn record_round_trip_and_encoded_checksum() -> Result<()> {
    let mut f = tempfile::NamedTempFile::new()?;
    let raw = vec![42; 32768];
    let (id, len) = record::write(&mut f, Kind::Chunk, &raw)?;
    assert!(len < raw.len() as u64);
    let (_, out) = record::read(f.as_file(), 0, Some(id))?;
    assert_eq!(&*out.bytes, &raw);
    f.seek(SeekFrom::Start(record::HEADER_LEN))?;
    f.write_all(&[0xff])?;
    assert!(record::read(f.as_file(), 0, Some(id)).is_err());
    Ok(())
}
#[test]
fn bounded_decoder_rejects_huge_declared_lengths() -> Result<()> {
    let mut f = tempfile::NamedTempFile::new()?;
    record::write(&mut f, Kind::Chunk, b"small")?;
    f.seek(SeekFrom::Start(16))?;
    f.write_all(&u64::MAX.to_le_bytes())?;
    assert!(record::header(f.as_file(), 0).is_err());
    Ok(())
}
#[test]
fn page_manifest_rejects_inconsistent_lengths() -> Result<()> {
    let mut e = Encoder::new();
    e.u32(4096);
    e.u32(100);
    e.u32(1);
    e.u8(1);
    e.id(Id::default());
    e.u32(99);
    assert!(Page::decode(&e.finish()?).is_err());
    Ok(())
}
#[test]
fn disk_index_binary_search_and_validation() -> Result<()> {
    let d = tempfile::tempdir()?;
    let mut out = IndexWriter::new(d.path())?;
    let mut ids: Vec<_> = (0u64..100).map(|n| Id::sha256(&n.to_be_bytes())).collect();
    ids.sort();
    for &id in &ids {
        out.push(Location {
            id,
            pack: Id::default(),
            offset: 8,
            length: 100,
            member: runinator_workspace_storage::index::STANDALONE,
        })?;
    }
    let f = out.finish()?;
    let index = DiskIndex::open(f.path())?;
    index.validate()?;
    for id in ids {
        assert_eq!(index.lookup(id)?.unwrap().id, id);
    }
    assert!(index.lookup(Id::sha256(b"absent"))?.is_none());
    Ok(())
}
#[test]
fn external_sort_spills_and_merges_multiple_runs() -> Result<()> {
    let d = tempfile::tempdir()?;
    let mut sort = ExternalSorter::new(d.path())?;
    let pack = Id::sha256(b"pack");
    for n in (0u64..10000).rev() {
        sort.push(Location {
            id: Id::sha256(&n.to_be_bytes()),
            pack: Id::default(),
            offset: n + 8,
            length: 100,
            member: runinator_workspace_storage::index::STANDALONE,
        })?;
    }
    let f = sort.finish(pack, d.path())?;
    let index = DiskIndex::open(f.path())?;
    assert_eq!(index.count, 10000);
    index.validate()?;
    for n in [0u64, 4000, 9999] {
        let loc = index.lookup(Id::sha256(&n.to_be_bytes()))?.unwrap();
        assert_eq!(loc.pack, pack);
        assert_eq!(loc.offset, n + 8);
    }
    Ok(())
}
#[test]
fn truncated_pack_is_rejected() -> Result<()> {
    let mut f = tempfile::NamedTempFile::new()?;
    f.write_all(record::PACK_MAGIC)?;
    record::write(&mut f, Kind::Chunk, b"x")?;
    let len = f.as_file().metadata()?.len();
    f.as_file().set_len(len - 1)?;
    assert!(record::visit_pack(f.path(), |_, _| Ok(())).is_err());
    Ok(())
}
