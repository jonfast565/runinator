#![cfg(unix)]
#![cfg(unix)]
use runinator_workspace_storage::{
    Config, Repository, Result,
    index::{DiskIndex, STANDALONE},
    model::{FileObject, Kind},
    oci, record,
    store::load,
};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

fn compacted_index(path: &Path) -> Result<DiskIndex> {
    let files = fs::read_dir(path.join("indexes"))?.collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(files.len(), 1, "GC must leave exactly the current index");
    DiskIndex::open(&files[0].path())
}

#[test]
fn tiny_files_share_physical_records_and_survive_gc_and_reopen() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let mut files = Vec::new();
    for n in 0..40u8 {
        let name = format!("file-{n:02}");
        let bytes = vec![n + 1; 600 + n as usize];
        let id = tx.put(&name, bytes.as_slice())?;
        files.push((name, id, bytes));
    }
    let revision = tx.commit("tiny file batch")?;
    repo.gc()?;
    let index = compacted_index(dir.path())?;
    let mut physical = BTreeSet::new();
    for (_, id, _) in &files {
        let loc = index.lookup(*id)?.unwrap();
        assert_ne!(loc.member, STANDALONE);
        physical.insert((loc.pack, loc.offset));
        let file = fs::File::open(dir.path().join("packs").join(format!("{}.pack", loc.pack)))?;
        assert_eq!(record::header(&file, loc.offset)?.kind, Kind::TinyBlock);
    }
    assert!(physical.len() < files.len());
    {
        let snap = repo.snapshot("main")?;
        for (name, id, bytes) in &files {
            assert_eq!(snap.file_id(name)?, *id);
            assert_eq!(snap.read_range(name, 0, bytes.len())?, *bytes);
        }
        assert_eq!(repo.page_cache().stats()?.resident_bytes, 0);
    }
    repo.fsck()?;
    drop(index);
    drop(repo);
    let repo = Repository::open(dir.path(), Config::default())?;
    assert_eq!(repo.head("main")?, Some(revision));
    for (name, id, bytes) in files {
        let snap = repo.snapshot("main")?;
        assert_eq!(snap.file_id(&name)?, id);
        assert_eq!(snap.read_range(&name, 0, bytes.len())?, bytes);
    }
    repo.fsck()?;
    Ok(())
}

#[test]
fn editing_one_packed_file_never_changes_its_neighbors_ids() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let a = tx.put("a", vec![1; 1000].as_slice())?;
    let b = tx.put("b", vec![2; 1000].as_slice())?;
    tx.hard_link("a", "alias")?;
    let old = tx.commit("before")?;
    let snapshot = repo.snapshot_at(old)?;
    let mut tx = repo.transaction("main")?;
    let new_a = tx.write("alias", 42, b"changed")?;
    tx.commit("after")?;
    let current = repo.snapshot("main")?;
    assert_ne!(a, new_a);
    assert_eq!(current.file_id("b")?, b);
    assert_eq!(current.file_id("a")?, new_a);
    assert_eq!(snapshot.file_id("a")?, a);
    assert_eq!(snapshot.read_range("a", 42, 7)?, vec![1; 7]);
    assert_eq!(current.read_range("alias", 42, 7)?, b"changed");
    drop(snapshot);
    drop(current);
    repo.gc()?;
    assert_eq!(repo.snapshot("main")?.file_id("b")?, b);
    repo.fsck()?;
    Ok(())
}

#[test]
fn gc_repacks_only_reachable_tiny_members() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let live = tx.put("live", vec![1; 1000].as_slice())?;
    let dead = tx.put("dead", vec![2; 1000].as_slice())?;
    tx.commit("initial")?;
    let mut tx = repo.transaction("main")?;
    tx.unlink("dead")?;
    tx.commit("delete")?;
    // History otherwise retains the old file intentionally.
    repo.checkpoint("main", "discard old history")?;
    repo.gc()?;
    let index = compacted_index(dir.path())?;
    assert!(index.lookup(dead)?.is_none());
    assert!(index.lookup(live)?.is_some());
    assert_eq!(
        repo.snapshot("main")?.read_range("live", 0, 1000)?,
        vec![1; 1000]
    );
    repo.fsck()?;
    Ok(())
}

#[test]
fn sized_ingestion_selects_large_pages_and_rejects_wrong_length_atomically() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    tx.put("file", b"old".as_slice())?;
    assert!(tx.put_sized("file", b"longer".as_slice(), 3).is_err());
    assert_eq!(tx.read_range("file", 0, 10)?, b"old");
    let size = 16 * 1024 * 1024;
    let id = tx.put_sized("large", std::io::repeat(1).take(size), size)?;
    tx.commit("sized ingest")?;
    let snap = repo.snapshot("main")?;
    let f: FileObject = load(&snap, id, Kind::File)?;
    assert_eq!(f.layout.page_size, 4 * 1024 * 1024);
    assert_eq!(f.size, size);
    drop(snap);
    repo.fsck()?;
    Ok(())
}

#[test]
fn sized_ingestion_checks_cache_budget_before_reading() -> Result<()> {
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("budget check should precede reads")
        }
    }
    let dir = tempfile::tempdir()?;
    let config = Config {
        page_cache_bytes: 4 * 1024 * 1024,
        ..Default::default()
    };
    let repo = Repository::init(dir.path(), config)?;
    let mut tx = repo.transaction("main")?;
    assert!(
        tx.put_sized("huge", MustNotRead, 1024 * 1024 * 1024)
            .is_err()
    );
    Ok(())
}

#[test]
fn native_oci_carries_tiny_blocks_without_changing_revision_identity() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path().join("src"), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let file = tx.put("tiny", vec![3; 600].as_slice())?;
    tx.put("other", vec![4; 800].as_slice())?;
    tx.hard_link("tiny", "alias")?;
    let rev = tx.commit("export")?;
    let layout = dir.path().join("native");
    oci::export_artifact(&repo.snapshot("main")?, &layout)?;
    let mut found_block = false;
    for entry in fs::read_dir(layout.join("blobs/sha256"))? {
        let mut file = fs::File::open(entry?.path())?;
        let mut prefix = [0; 8];
        if file.read_exact(&mut prefix).is_err() || &prefix != record::PACK_MAGIC {
            continue;
        }
        let mut pos = 8;
        while pos < file.metadata()?.len() {
            let header = record::header(&file, pos)?;
            found_block |= header.kind == Kind::TinyBlock;
            pos += header.record_len()?;
        }
    }
    assert!(found_block);
    let dest = Repository::init(dir.path().join("dest"), Config::default())?;
    assert_eq!(oci::import_artifact(&dest, &layout, "main", None)?, rev);
    let snap = dest.snapshot("main")?;
    assert_eq!(snap.file_id("tiny")?, file);
    assert_eq!(snap.stat("tiny")?.0, snap.stat("alias")?.0);
    assert_eq!(snap.read_range("alias", 0, 600)?, vec![3; 600]);
    drop(snap);
    dest.fsck()?;
    dest.gc()?;
    Ok(())
}

#[test]
fn merkle_oci_roundtrip_handles_tiny_edits_and_zero_extents() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path().join("src"), Config::default())?;
    let mut tx = repo.transaction("main")?;
    tx.put("tiny", vec![1; 500].as_slice())?;
    let mut bytes = vec![0; 200000];
    bytes[..300].fill(7);
    bytes[199900..].fill(8);
    tx.put("sparse", bytes.as_slice())?;
    tx.commit("base")?;
    let mut tx = repo.transaction("main")?;
    tx.write("tiny", 1, b"delta")?;
    tx.punch_hole("sparse", 199920, 40)?;
    tx.commit("delta")?;
    bytes[199920..199960].fill(0);
    let layout = dir.path().join("image");
    oci::export_image_merkle(
        &repo.snapshot("main")?,
        &layout,
        &oci::MerkleImageOptions::default(),
    )?;
    let dest = Repository::init(dir.path().join("dest"), Config::default())?;
    oci::import_image(&dest, &layout, "main", "import v0.8 image")?;
    let snap = dest.snapshot("main")?;
    assert_eq!(snap.read_range("tiny", 1, 5)?, b"delta");
    assert_eq!(snap.read_range("sparse", 0, bytes.len())?, bytes);
    drop(snap);
    dest.fsck()?;
    Ok(())
}

#[test]
fn fastcdc_chunks_are_physically_grouped_and_round_trip_after_gc() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let a = b"abcdefghijklmnopqrstuvwxyz0123456789".repeat(20_000);
    let mut b = a.clone();
    b[120_000..120_256].fill(b'!');
    tx.put("a.bin", a.as_slice())?;
    tx.put("b.bin", b.as_slice())?;
    tx.commit("group chunks")?;
    repo.gc()?;
    let index = compacted_index(dir.path())?;
    let snap = repo.snapshot("main")?;
    let f: FileObject = load(&snap, snap.file_id("a.bin")?, Kind::File)?;
    let page =
        runinator_workspace_storage::pages::page_id(&snap, snap.file_id("a.bin")?, 0)?.unwrap();
    let p: runinator_workspace_storage::model::Page = load(&snap, page, Kind::Page)?;
    let chunk = p
        .extents
        .iter()
        .find_map(|e| match e {
            runinator_workspace_storage::model::PageExtent::Data(c) => Some(c.id),
            _ => None,
        })
        .unwrap();
    let loc = index.lookup(chunk)?.unwrap();
    assert_ne!(loc.member, STANDALONE);
    let file = fs::File::open(dir.path().join("packs").join(format!("{}.pack", loc.pack)))?;
    assert_eq!(record::header(&file, loc.offset)?.kind, Kind::ChunkBlock);
    assert_eq!(snap.read_range("a.bin", 0, a.len())?, a);
    assert_eq!(snap.read_range("b.bin", 0, b.len())?, b);
    drop(f);
    Ok(())
}
