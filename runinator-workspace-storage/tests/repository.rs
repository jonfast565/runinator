#![cfg(unix)]
#![cfg(unix)]
use runinator_workspace_storage::{
    CommitPoint, Config, Error, Id, Layout, Repository, Result,
    model::Kind,
    oci,
    projection::{self, PathNode, PathProjection},
    store::load,
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};
fn config() -> Config {
    Config {
        layout: Layout {
            page_size: 4096,
            min: 64,
            avg: 256,
            max: 1024,
        },
        page_cache_bytes: 4 * 4096,
        metadata_cache_bytes: 1024 * 1024,
        chunk_cache_bytes: 1024 * 1024,
    }
}
fn initialize(path: &Path) -> Result<(Repository, Id)> {
    let repo = Repository::init(path, config())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir_all("src")?;
    tx.put("src/a", b"original".as_slice())?;
    let id = tx.commit("initial")?;
    Ok((repo, id))
}
#[test]
fn committed_data_survives_reopen() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, id) = initialize(d.path())?;
    drop(repo);
    let repo = Repository::open(d.path(), config())?;
    assert_eq!(repo.head("main")?, Some(id));
    assert_eq!(
        repo.snapshot("main")?.read_range("src/a", 0, 100)?,
        b"original"
    );
    assert!(repo.fsck()? > 0);
    Ok(())
}
#[test]
fn aborted_transaction_never_publishes_objects() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    {
        let mut tx = repo.transaction("main")?;
        tx.put("a", b"discard me".as_slice())?;
    }
    assert_eq!(repo.head("main")?, None);
    assert_eq!(repo.stats()?.objects, 0);
    Ok(())
}
#[test]
fn optimistic_writers_detect_conflict() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut a = repo.transaction("main")?;
    let mut b = repo.transaction("main")?;
    a.write("src/a", 0, b"A")?;
    b.write("src/a", 0, b"B")?;
    let id = a.commit("A")?;
    assert!(matches!(b.commit("B"), Err(Error::Conflict)));
    assert_eq!(repo.head("main")?, Some(id));
    assert_eq!(repo.snapshot("main")?.read_range("src/a", 0, 1)?, b"A");
    Ok(())
}
#[test]
fn independent_branches_do_not_lose_catalog_updates() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    let mut a = repo.transaction("a")?;
    let mut b = repo.transaction("b")?;
    a.put("x", b"same".as_slice())?;
    b.put("x", b"same".as_slice())?;
    a.commit("a")?;
    b.commit("b")?;
    assert_eq!(repo.refs()?.len(), 2);
    assert_eq!(
        repo.snapshot("a")?.file_id("x")?,
        repo.snapshot("b")?.file_id("x")?
    );
    Ok(())
}
#[test]
fn concurrent_thread_transactions_are_supported() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Arc::new(Repository::init(d.path(), config())?);
    let mut handles = Vec::new();
    for i in 0..4 {
        let r = repo.clone();
        handles.push(std::thread::spawn(move || -> Result<Id> {
            let mut tx = r.transaction(&format!("branch-{i}"))?;
            tx.put("x", b"data".as_slice())?;
            tx.commit("thread")
        }));
    }
    for h in handles {
        h.join().unwrap()?;
    }
    assert_eq!(repo.refs()?.len(), 4);
    repo.fsck()?;
    Ok(())
}
#[test]
fn same_repository_cannot_be_opened_twice() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    assert!(matches!(
        Repository::open(d.path(), config()),
        Err(Error::Busy(_))
    ));
    drop(repo);
    Repository::open(d.path(), config())?;
    Ok(())
}
#[test]
fn active_snapshot_keeps_old_revision_and_blocks_gc() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let old = repo.snapshot("main")?;
    let mut tx = repo.transaction("main")?;
    tx.write("src/a", 0, b"NEW")?;
    tx.commit("changed")?;
    assert_eq!(old.read_range("src/a", 0, 100)?, b"original");
    assert!(matches!(repo.gc(), Err(Error::Busy(_))));
    drop(old);
    repo.gc()?;
    Ok(())
}
#[test]
fn active_transaction_blocks_gc() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let tx = repo.transaction("main")?;
    assert!(matches!(repo.gc(), Err(Error::Busy(_))));
    drop(tx);
    repo.gc()?;
    Ok(())
}
#[test]
fn read_through_page_cache_is_used() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.put("src/a", std::io::repeat(42).take(70000))?;
    tx.commit("paged fixture, not a tiny leaf")?;
    let s = repo.snapshot("main")?;
    s.read_range("src/a", 0, 5)?;
    let before = repo.page_cache().stats()?.hits;
    s.read_range("src/a", 0, 5)?;
    assert!(repo.page_cache().stats()?.hits > before);
    Ok(())
}
#[test]
fn hard_links_share_mutation_but_old_snapshot_is_immutable() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.hard_link("src/a", "src/b")?;
    tx.commit("link")?;
    let old = repo.snapshot("main")?;
    assert_eq!(old.stat("src/a")?.0, old.stat("src/b")?.0);
    assert_eq!(old.stat("src/a")?.1.links, 2);
    let mut tx = repo.transaction("main")?;
    tx.write("src/b", 0, b"NEW")?;
    tx.commit("modify")?;
    assert_eq!(repo.snapshot("main")?.read_range("src/a", 0, 3)?, b"NEW");
    assert_eq!(old.read_range("src/a", 0, 8)?, b"original");
    drop(old);
    repo.fsck()?;
    Ok(())
}
#[test]
fn unlinking_hard_link_preserves_other_name() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.hard_link("src/a", "b")?;
    tx.unlink("src/a")?;
    tx.commit("unlink one")?;
    let s = repo.snapshot("main")?;
    assert_eq!(s.stat("b")?.1.links, 1);
    assert_eq!(s.read_range("b", 0, 8)?, b"original");
    drop(s);
    repo.fsck()?;
    Ok(())
}
#[test]
fn rename_between_directories_and_replace_work() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir("dest")?;
    tx.put("dest/b", b"old target".as_slice())?;
    tx.rename("src/a", "dest/b", true)?;
    tx.rename("dest/b", "dest/c", false)?;
    tx.commit("rename")?;
    let s = repo.snapshot("main")?;
    assert!(s.stat("src/a").is_err());
    assert_eq!(s.read_range("dest/c", 0, 8)?, b"original");
    drop(s);
    repo.fsck()?;
    Ok(())
}
#[test]
fn directory_rename_does_not_rewrite_descendant_files() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let old = repo.snapshot("main")?.file_id("src/a")?;
    let mut tx = repo.transaction("main")?;
    tx.rename("src", "renamed", false)?;
    tx.commit("directory rename")?;
    assert_eq!(repo.snapshot("main")?.file_id("renamed/a")?, old);
    Ok(())
}
#[test]
fn failed_namespace_operation_rolls_back_transaction_roots() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir("src/child")?;
    assert!(tx.rename("src", "src/child/invalid", false).is_err());
    assert_eq!(tx.read_range("src/a", 0, 8)?, b"original");
    assert!(tx.unlink("src").is_err());
    tx.commit("still valid")?;
    repo.fsck()?;
    Ok(())
}
struct BrokenReader {
    first: bool,
}
impl Read for BrokenReader {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if !self.first {
            return Err(std::io::Error::other("simulated source failure"));
        }
        self.first = false;
        let n = b.len().min(3);
        b[..n].fill(b'X');
        Ok(n)
    }
}
#[test]
fn streaming_write_error_does_not_partially_change_namespace() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    assert!(
        tx.write_from("src/a", 0, BrokenReader { first: true })
            .is_err()
    );
    assert_eq!(tx.read_range("src/a", 0, 8)?, b"original");
    tx.commit("after failed operation")?;
    repo.fsck()?;
    Ok(())
}
#[test]
fn symlinks_are_explicit_and_confined_to_virtual_root() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    tx.symlink("src/link", "a")?;
    tx.symlink("absolute", "/src/a")?;
    tx.symlink("escape", "../../outside")?;
    tx.symlink("loop", "loop")?;
    tx.commit("symlinks")?;
    let s = repo.snapshot("main")?;
    assert_eq!(s.read_link("src/link")?, "a");
    assert!(s.file_id("src/link").is_err());
    assert_eq!(s.file_id_follow("src/link")?, s.file_id("src/a")?);
    assert_eq!(s.file_id_follow("absolute")?, s.file_id("src/a")?);
    assert!(s.file_id_follow("escape").is_err());
    assert!(s.file_id_follow("loop").is_err());
    drop(s);
    repo.fsck()?;
    Ok(())
}
#[test]
fn invalid_paths_are_rejected_without_side_effects() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let mut tx = repo.transaction("main")?;
    for path in [
        "../oops",
        "src//oops",
        "src/./oops",
        "src\\oops",
        "src/../oops",
    ] {
        assert!(tx.put(path, b"x".as_slice()).is_err());
    }
    assert_eq!(tx.read_range("src/a", 0, 8)?, b"original");
    Ok(())
}
#[test]
fn metadata_and_xattrs_round_trip_without_changing_file_content() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    let before = repo.snapshot("main")?.file_id("src/a")?;
    let mut tx = repo.transaction("main")?;
    let mut meta = tx.stat("src/a")?.1.metadata;
    meta.mode = 0o600;
    meta.modified_ns = 123456;
    tx.set_metadata("src/a", meta)?;
    tx.set_xattr("src/a", "user.note", Some(b"abc"))?;
    tx.commit("metadata")?;
    drop(repo);
    let repo = Repository::open(d.path(), config())?;
    let s = repo.snapshot("main")?;
    assert_eq!(s.file_id("src/a")?, before);
    let i = s.stat("src/a")?.1;
    assert_eq!(i.metadata.mode, 0o600);
    assert_eq!(i.metadata.modified_ns, 123456);
    assert_eq!(i.metadata.xattrs["user.note"], b"abc");
    Ok(())
}
#[test]
fn checkpoint_and_gc_respect_other_retained_refs() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, old) = initialize(d.path())?;
    repo.update_ref_if("keep", None, Some(old))?;
    let mut tx = repo.transaction("main")?;
    tx.unlink("src/a")?;
    tx.commit("delete")?;
    let checkpoint = repo.checkpoint("main", "forget parent history")?;
    repo.gc()?;
    assert_eq!(
        repo.snapshot_at(old)?.read_range("src/a", 0, 8)?,
        b"original"
    );
    repo.update_ref_if("keep", Some(old), None)?;
    let report = repo.gc()?;
    assert!(report.removed_objects > 0);
    assert!(repo.snapshot_at(old).is_err());
    assert_eq!(repo.head("main")?, Some(checkpoint));
    repo.fsck()?;
    Ok(())
}
#[test]
fn gc_compacts_packs_without_changing_revision_identity() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    for n in 0..3 {
        let mut tx = repo.transaction("main")?;
        tx.write("src/a", 0, &[b'A' + n])?;
        tx.commit("update")?;
    }
    let head = repo.head("main")?;
    let report = repo.gc()?;
    assert_eq!(report.packs_after, 1);
    assert_eq!(repo.head("main")?, head);
    drop(repo);
    let repo = Repository::open(d.path(), config())?;
    assert_eq!(repo.head("main")?, head);
    repo.fsck()?;
    Ok(())
}
#[test]
fn empty_repository_gc_is_valid() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    let report = repo.gc()?;
    assert_eq!(report.live_objects, 0);
    assert_eq!(report.packs_after, 0);
    drop(repo);
    Repository::open(d.path(), config())?.fsck()?;
    Ok(())
}
#[test]
fn publication_failure_boundaries_preserve_a_valid_generation() -> Result<()> {
    for point in [
        CommitPoint::AfterPack,
        CommitPoint::AfterIndex,
        CommitPoint::BeforeCurrent,
        CommitPoint::AfterCurrent,
    ] {
        let d = tempfile::tempdir()?;
        let (repo, old) = initialize(d.path())?;
        let mut tx = repo.transaction("main")?;
        tx.write("src/a", 0, b"NEW")?;
        repo.inject_failure_once(point);
        assert!(tx.commit("injected").is_err());
        drop(repo);
        let repo = Repository::open(d.path(), config())?;
        let bytes = repo.snapshot("main")?.read_range("src/a", 0, 8)?;
        if matches!(point, CommitPoint::AfterCurrent) {
            assert_ne!(repo.head("main")?, Some(old));
            assert_eq!(bytes, b"NEWginal");
        } else {
            assert_eq!(repo.head("main")?, Some(old));
            assert_eq!(bytes, b"original");
        }
        repo.fsck()?;
    }
    Ok(())
}
#[test]
fn failed_gc_does_not_delete_previous_generation_packs() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, old) = initialize(d.path())?;
    repo.inject_failure_once(CommitPoint::BeforeCurrent);
    assert!(repo.gc().is_err());
    drop(repo);
    let repo = Repository::open(d.path(), config())?;
    assert_eq!(repo.head("main")?, Some(old));
    assert_eq!(
        repo.snapshot("main")?.read_range("src/a", 0, 8)?,
        b"original"
    );
    repo.fsck()?;
    Ok(())
}
#[test]
fn abandoned_temp_files_are_cleaned_on_open() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    fs::create_dir(d.path().join("tmp/abandoned"))?;
    fs::write(d.path().join("tmp/abandoned/partial"), b"not published")?;
    drop(repo);
    let repo = Repository::open(d.path(), config())?;
    assert!(!d.path().join("tmp/abandoned").exists());
    repo.fsck()?;
    Ok(())
}
fn corrupt_one(path: &Path, offset: u64) -> Result<()> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut b = [0];
    f.read_exact(&mut b)?;
    b[0] ^= 255;
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(&b)?;
    f.sync_all()?;
    Ok(())
}
#[test]
fn corrupt_pack_is_detected_even_when_read_cache_is_warm() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    repo.snapshot("main")?.read_range("src/a", 0, 8)?;
    let path = fs::read_dir(d.path().join("packs"))?
        .next()
        .unwrap()?
        .path();
    corrupt_one(&path, 100)?;
    assert!(repo.fsck().is_err());
    Ok(())
}
#[test]
fn corrupt_index_is_rejected_instead_of_silently_resetting_repo() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(d.path())?;
    drop(repo);
    let path = fs::read_dir(d.path().join("indexes"))?
        .next()
        .unwrap()?
        .path();
    corrupt_one(&path, 20)?;
    assert!(Repository::open(d.path(), config()).is_err());
    assert!(d.path().join("CURRENT").exists());
    Ok(())
}
#[test]
fn oci_round_trip_preserves_revision_id_and_hard_links() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, _) = initialize(&d.path().join("source"))?;
    let mut tx = repo.transaction("main")?;
    tx.hard_link("src/a", "copy")?;
    tx.symlink("symlink", "src/a")?;
    let id = tx.commit("export me")?;
    let destination = d.path().join("layout");
    oci::export(&repo.snapshot("main")?, &destination)?;
    assert!(destination.join("oci-layout").exists());
    assert!(destination.join("index.json").exists());
    let imported = Repository::init(d.path().join("imported"), config())?;
    assert_eq!(oci::import(&imported, &destination, "main", None)?, id);
    let s = imported.snapshot("main")?;
    assert_eq!(s.read_range("copy", 0, 8)?, b"original");
    assert_eq!(s.stat("copy")?.0, s.stat("src/a")?.0);
    assert_eq!(s.read_link("symlink")?, "src/a");
    drop(s);
    imported.fsck()?;
    Ok(())
}
#[test]
fn oci_corruption_does_not_publish_destination_ref() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (source, _) = initialize(&d.path().join("source"))?;
    let layout = d.path().join("layout");
    oci::export(&source.snapshot("main")?, &layout)?;
    for file in fs::read_dir(layout.join("blobs/sha256"))? {
        let file = file?;
        let mut f = std::fs::File::open(file.path())?;
        let mut prefix = [0; 8];
        if f.read_exact(&mut prefix).is_ok()
            && &prefix == runinator_workspace_storage::record::PACK_MAGIC
        {
            corrupt_one(&file.path(), 100)?;
        }
    }
    let dest = Repository::init(d.path().join("dest"), config())?;
    assert!(oci::import(&dest, &layout, "main", None).is_err());
    assert_eq!(dest.head("main")?, None);
    Ok(())
}
#[test]
fn import_and_ref_updates_require_expected_old_value() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (repo, id) = initialize(d.path())?;
    assert!(matches!(
        repo.update_ref_if("main", None, Some(id)),
        Err(Error::Conflict)
    ));
    assert!(repo.update_ref_if("../invalid", None, Some(id)).is_err());
    Ok(())
}

#[test]
fn standard_oci_image_roundtrip_preserves_regular_symlink_and_hardlink() -> Result<()> {
    let d = tempfile::tempdir()?;
    let source_dir = d.path().join("source");
    let source = Repository::init(&source_dir, config())?;
    let mut tx = source.transaction("main")?;
    tx.mkdir_all("bin")?;
    tx.put("bin/tool", b"hello-oci".as_slice())?;
    tx.hard_link("bin/tool", "bin/tool-hard")?;
    tx.symlink("bin/tool-link", "tool")?;
    tx.commit("oci source")?;

    let layout = d.path().join("image-layout");
    let snapshot = source.snapshot("main")?;
    oci::export_image(&snapshot, &layout, &oci::ImageOptions::default())?;
    drop(snapshot);

    assert!(layout.join("oci-layout").is_file());
    assert!(layout.join("index.json").is_file());

    let imported_dir = d.path().join("imported");
    let imported = Repository::init(&imported_dir, config())?;
    oci::import_image(&imported, &layout, "main", "import OCI image")?;
    let s = imported.snapshot("main")?;
    assert_eq!(s.read_range("bin/tool", 0, 64)?, b"hello-oci");
    assert_eq!(s.read_range("bin/tool-hard", 0, 64)?, b"hello-oci");
    assert_eq!(s.stat("bin/tool")?.0, s.stat("bin/tool-hard")?.0);
    assert_eq!(s.read_link("bin/tool-link")?, "tool");
    Ok(())
}

#[test]
fn standard_oci_image_import_requires_fresh_ref() -> Result<()> {
    let d = tempfile::tempdir()?;
    let (source, _) = initialize(&d.path().join("source"))?;
    let layout = d.path().join("image-layout");
    oci::export_image(
        &source.snapshot("main")?,
        &layout,
        &oci::ImageOptions::default(),
    )?;

    let (dest, _) = initialize(&d.path().join("dest"))?;
    assert!(matches!(
        oci::import_image(&dest, &layout, "main", "should fail"),
        Err(Error::Exists(_))
    ));
    Ok(())
}

fn oci_manifest_layer_count(layout: &Path) -> Result<usize> {
    let index: serde_json::Value = serde_json::from_slice(&fs::read(layout.join("index.json"))?)?;
    let digest = index["manifests"][0]["digest"]
        .as_str()
        .ok_or_else(|| Error::Invalid("missing OCI manifest digest".into()))?;
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| Error::Invalid("bad OCI digest".into()))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(layout.join("blobs/sha256").join(hex))?)?;
    Ok(manifest["layers"]
        .as_array()
        .ok_or_else(|| Error::Invalid("missing OCI layers".into()))?
        .len())
}

#[test]
fn merkle_oci_history_roundtrip_applies_deltas_whiteouts_and_hardlinks() -> Result<()> {
    let d = tempfile::tempdir()?;
    let source = Repository::init(d.path().join("source"), config())?;

    let mut tx = source.transaction("main")?;
    tx.mkdir_all("app")?;
    tx.put("app/a", b"revision-one".as_slice())?;
    tx.put("app/gone", b"delete-me".as_slice())?;
    tx.commit("r1")?;

    let mut tx = source.transaction("main")?;
    tx.write("app/a", 0, b"REVISION")?;
    tx.unlink("app/gone")?;
    tx.hard_link("app/a", "app/a-hard")?;
    tx.put("app/new", b"new-file".as_slice())?;
    tx.commit("r2")?;

    let mut tx = source.transaction("main")?;
    tx.write("app/new", 0, b"NEW")?;
    tx.commit("r3")?;

    let layout = d.path().join("merkle-image");
    let snapshot = source.snapshot("main")?;
    let options = oci::MerkleImageOptions {
        max_layers: 3,
        ..Default::default()
    };
    oci::export_image_merkle(&snapshot, &layout, &options)?;
    drop(snapshot);
    assert_eq!(oci_manifest_layer_count(&layout)?, 3);

    let imported = Repository::init(d.path().join("imported"), config())?;
    oci::import_image(&imported, &layout, "main", "import merkle OCI")?;
    let s = imported.snapshot("main")?;
    assert_eq!(s.read_range("app/a", 0, 64)?, b"REVISION-one");
    assert_eq!(s.read_range("app/new", 0, 64)?, b"NEW-file");
    assert!(matches!(s.stat("app/gone"), Err(Error::NotFound(_))));
    assert_eq!(s.stat("app/a")?.0, s.stat("app/a-hard")?.0);
    Ok(())
}

#[test]
fn merkle_oci_history_bounds_layers_with_checkpoint() -> Result<()> {
    let d = tempfile::tempdir()?;
    let source = Repository::init(d.path().join("source"), config())?;
    let mut tx = source.transaction("main")?;
    tx.put("value", b"0000".as_slice())?;
    tx.commit("r0")?;
    for byte in *b"1234" {
        let mut tx = source.transaction("main")?;
        tx.write("value", 0, &[byte])?;
        tx.commit("next")?;
    }

    let layout = d.path().join("bounded-image");
    let snapshot = source.snapshot("main")?;
    oci::export_image_merkle(
        &snapshot,
        &layout,
        &oci::MerkleImageOptions {
            max_layers: 2,
            ..Default::default()
        },
    )?;
    drop(snapshot);
    assert_eq!(oci_manifest_layer_count(&layout)?, 2);

    let imported = Repository::init(d.path().join("imported"), config())?;
    oci::import_image(&imported, &layout, "main", "bounded")?;
    assert_eq!(
        imported.snapshot("main")?.read_range("value", 0, 8)?,
        b"4000"
    );
    Ok(())
}

#[test]
fn revision_path_projection_reuses_unchanged_subtrees() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir_all("left")?;
    tx.mkdir_all("right")?;
    tx.put("left/a", b"alpha".as_slice())?;
    tx.put("right/b", b"beta".as_slice())?;
    let r1 = tx.commit("r1")?;

    let mut tx = repo.transaction("main")?;
    tx.write("left/a", 0, b"A")?;
    let r2 = tx.commit("r2")?;

    let s1 = repo.snapshot_at(r1)?;
    let p1: PathProjection = load(&s1, s1.revision.projection, Kind::PathProjection)?;
    let root1: PathNode = load(&s1, p1.root, Kind::PathNode)?;
    let left1 = root1.child(&s1, "left")?.unwrap();
    let right1 = root1.child(&s1, "right")?.unwrap();
    drop(s1);

    let s2 = repo.snapshot_at(r2)?;
    let p2: PathProjection = load(&s2, s2.revision.projection, Kind::PathProjection)?;
    let root2: PathNode = load(&s2, p2.root, Kind::PathNode)?;
    assert_ne!(p1.root, p2.root);
    assert_ne!(left1, root2.child(&s2, "left")?.unwrap());
    assert_eq!(right1, root2.child(&s2, "right")?.unwrap());
    Ok(())
}

#[test]
fn path_projection_indexes_hardlink_aliases() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir_all("a")?;
    tx.mkdir_all("b")?;
    tx.put("a/file", b"shared".as_slice())?;
    tx.hard_link("a/file", "b/alias")?;
    let revision = tx.commit("links")?;

    let snapshot = repo.snapshot_at(revision)?;
    let inode = snapshot.stat("a/file")?.0;
    let p: PathProjection = load(
        &snapshot,
        snapshot.revision.projection,
        Kind::PathProjection,
    )?;
    assert_eq!(
        projection::hardlink_paths(&snapshot, &p, inode)?,
        vec!["a/file".to_string(), "b/alias".to_string()]
    );
    Ok(())
}

#[test]
fn maintained_projection_tracks_mutations_and_directory_moves() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;

    let mut tx = repo.transaction("main")?;
    tx.mkdir_all("a/sub")?;
    tx.mkdir_all("b")?;
    tx.put("a/sub/file", b"before".as_slice())?;
    tx.hard_link("a/sub/file", "b/alias")?;
    tx.commit("base")?;

    let mut tx = repo.transaction("main")?;
    tx.write("b/alias", 0, b"after!")?;
    tx.rename("a", "moved", false)?;
    let revision = tx.commit("incremental projection")?;

    let snapshot = repo.snapshot_at(revision)?;
    assert_eq!(
        snapshot.read_range("moved/sub/file", 0, 6)?,
        b"after!".to_vec()
    );
    assert_eq!(snapshot.read_range("b/alias", 0, 6)?, b"after!".to_vec());
    let inode = snapshot.stat("moved/sub/file")?.0;
    let p: PathProjection = load(
        &snapshot,
        snapshot.revision.projection,
        Kind::PathProjection,
    )?;
    assert_eq!(
        projection::hardlink_paths(&snapshot, &p, inode)?,
        vec!["b/alias".to_string(), "moved/sub/file".to_string()]
    );
    drop(snapshot);
    repo.fsck()?;

    let mut tx = repo.transaction("main")?;
    tx.unlink("b/alias")?;
    let revision = tx.commit("drop alias")?;
    let snapshot = repo.snapshot_at(revision)?;
    let inode = snapshot.stat("moved/sub/file")?.0;
    let p: PathProjection = load(
        &snapshot,
        snapshot.revision.projection,
        Kind::PathProjection,
    )?;
    assert!(projection::hardlink_paths(&snapshot, &p, inode)?.is_empty());
    drop(snapshot);
    repo.fsck()?;
    Ok(())
}

#[test]
fn structural_hardlink_refs_survive_ancestor_directory_move() -> Result<()> {
    let d = tempfile::tempdir()?;
    let repo = Repository::init(d.path(), config())?;
    let mut tx = repo.transaction("main")?;
    tx.mkdir_all("a/sub/deep")?;
    tx.mkdir_all("b")?;
    tx.put("a/sub/deep/file", b"shared".as_slice())?;
    tx.hard_link("a/sub/deep/file", "b/alias")?;
    let before = tx.commit("before move")?;

    let snapshot = repo.snapshot_at(before)?;
    let inode = snapshot.stat("a/sub/deep/file")?.0;
    let p: PathProjection = load(
        &snapshot,
        snapshot.revision.projection,
        Kind::PathProjection,
    )?;
    let before_refs = projection::hardlink_refs(&snapshot, &p, inode)?;
    assert_eq!(before_refs.len(), 2);
    drop(snapshot);

    let mut tx = repo.transaction("main")?;
    tx.rename("a", "moved", false)?;
    let after = tx.commit("move ancestor")?;

    let snapshot = repo.snapshot_at(after)?;
    let p: PathProjection = load(
        &snapshot,
        snapshot.revision.projection,
        Kind::PathProjection,
    )?;
    let after_refs = projection::hardlink_refs(&snapshot, &p, inode)?;
    assert_eq!(
        before_refs, after_refs,
        "ancestor move must not rewrite descendant structural refs"
    );
    assert_eq!(
        projection::hardlink_paths(&snapshot, &p, inode)?,
        vec!["b/alias".to_string(), "moved/sub/deep/file".to_string()]
    );
    drop(snapshot);
    repo.fsck()?;
    Ok(())
}
