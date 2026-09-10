//! Provider directory and named result integration.
use super::*;
use storage::{
    staging::{EmptyStore, Staging},
    view::View,
};

#[test]
fn native_layout_contains_selected_revision_and_results() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("file"), b"native")?;
    let results = BTreeMap::from([("answer".into(), Value::from(42))]);
    let (edit, expected) = capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &results,
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let revision = edit.finish("native", None)?;
    let mut archive = Vec::new();
    crate::native::export(&edit.store, revision, scratch.path(), &mut archive)?;
    let (store, imported, usage) = crate::native::import(
        archive.as_slice(),
        scratch.path(),
        WorkspaceLimits::default(),
    )?;
    assert_eq!(imported, revision);
    assert_eq!(usage, expected);
    let view = View::new(&store, imported)?;
    assert_eq!(read_results(&view)?, results);
    assert_eq!(view.read_range("file", 1, 3)?, b"ati");
    let (packed, imported, packed_usage) = crate::native::import_packed(
        archive.as_slice(),
        scratch.path(),
        WorkspaceLimits::default(),
    )?;
    assert_eq!(packed_usage, expected);
    let packed = View::new(&packed, imported)?;
    assert_eq!(read_results(&packed)?, results);
    assert_eq!(packed.read_range("file", 1, 3)?, b"ati");
    Ok(())
}

#[test]
fn capture_materialize_and_exact_limits() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::create_dir(source.path().join("empty"))?;
    fs::write(source.path().join("file"), b"abc")?;
    let results = BTreeMap::from([("name".into(), Value::from("value"))]);
    let bytes = serde_json::to_vec(&results)?.len() as u64;
    let limits = WorkspaceLimits {
        max_bytes: bytes + 3,
        max_entries: 2,
        max_results_bytes: bytes,
    };
    let store = Staging::new(EmptyStore, scratch.path())?;
    let (edit, accounted) = capture(store, source.path(), &results, limits, scratch.path())?;
    let revision = edit.finish("capture", None)?;
    storage::gc::verify_roots(&edit.store, &[revision], scratch.path(), false)?;
    let view = View::new(&edit.store, revision)?;
    assert_eq!(usage(&view)?, accounted);
    assert_eq!(read_results(&view)?, results);
    assert!(
        WorkspaceLimits {
            max_bytes: limits.max_bytes - 1,
            ..limits
        }
        .check(accounted)
        .is_err()
    );
    let output = tempfile::tempdir()?;
    materialize(&view, output.path())?;
    assert!(output.path().join("empty").is_dir());
    assert_eq!(fs::read(output.path().join("file"))?, b"abc");
    Ok(())
}

#[test]
fn result_checkpoint_reuses_the_base_namespace() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("file"), b"unchanged")?;
    let (base_edit, base_usage) = capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let base = base_edit.finish("base", None)?;
    let results = BTreeMap::from([("result".into(), Value::from("checkpoint"))]);
    let (checkpoint, expected) = checkpoint_results(
        Staging::new_deduplicating(&base_edit.store, scratch.path())?,
        base,
        base_usage,
        &results,
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    assert_eq!(checkpoint.workspace.inodes, base_edit.workspace.inodes);
    assert_eq!(checkpoint.projection.root, base_edit.projection.root);
    let revision = checkpoint.finish("checkpoint", Some(base))?;
    let view = View::new(&checkpoint.store, revision)?;
    assert_eq!(read_results(&view)?, results);
    assert_eq!(usage(&view)?, expected);
    Ok(())
}

#[test]
fn results_larger_than_a_single_storage_object() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    let value = Value::from("x".repeat(17 * 1024 * 1024));
    let results = BTreeMap::from([("large".into(), value)]);
    let limits = WorkspaceLimits {
        max_results_bytes: 20 * 1024 * 1024,
        ..Default::default()
    };
    let store = Staging::new(EmptyStore, scratch.path())?;
    let (edit, expected) = capture(store, source.path(), &results, limits, scratch.path())?;
    let view = View::new(&edit.store, edit.finish("large", None)?)?;
    assert_eq!(usage(&view)?, expected);
    assert_eq!(read_result(&view, "large")?, results["large"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn hardlinks_count_once_and_restore_as_aliases() -> Result<(), SendableError> {
    use std::os::unix::fs::MetadataExt;
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("a"), b"abc")?;
    fs::hard_link(source.path().join("a"), source.path().join("b"))?;
    std::os::unix::fs::symlink("a", source.path().join("c"))?;
    let store = Staging::new(EmptyStore, scratch.path())?;
    let (edit, expected) = capture(
        store,
        source.path(),
        &BTreeMap::new(),
        WorkspaceLimits {
            max_bytes: 5,
            max_entries: 3,
            max_results_bytes: 2,
        },
        scratch.path(),
    )?;
    let view = View::new(&edit.store, edit.finish("aliases", None)?)?;
    assert_eq!(usage(&view)?, expected);
    let output = tempfile::tempdir()?;
    materialize(&view, output.path())?;
    assert_eq!(
        fs::metadata(output.path().join("a"))?.ino(),
        fs::metadata(output.path().join("b"))?.ino()
    );
    assert_eq!(fs::read(output.path().join("c"))?, b"abc");
    Ok(())
}

#[test]
fn reconciliation_preserves_unaffected_identity_and_noop_roots() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("stable"), "content")?;
    let stage = Staging::new(EmptyStore, scratch.path())?;
    let (first, _) = capture(
        stage,
        source.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let base = first.finish("first", None)?;
    let before = View::new(&first.store, base)?;
    let output = tempfile::tempdir()?;
    materialize(&before, output.path())?;
    let stage = Staging::new(&first.store, scratch.path())?;
    let (same, _) = capture_from(
        stage,
        Some(base),
        output.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    assert_eq!(same.workspace.inodes, first.workspace.inodes);
    assert_eq!(same.projection.root, first.projection.root);
    fs::write(output.path().join("aaa-new"), "new")?;
    let stage = Staging::new(&first.store, scratch.path())?;
    let (changed, _) = capture_from(
        stage,
        Some(base),
        output.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    assert_eq!(changed.stat("stable")?.0, before.stat("stable")?.0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn split_hardlinks_do_not_modify_the_remaining_alias() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("a"), "original")?;
    fs::hard_link(source.path().join("a"), source.path().join("b"))?;
    let (first, _) = capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let base = first.finish("links", None)?;
    fs::remove_file(source.path().join("a"))?;
    fs::write(source.path().join("a"), "replacement")?;
    let (changed, _) = capture_from(
        Staging::new(&first.store, scratch.path())?,
        Some(base),
        source.path(),
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let view = View::new(&changed.store, changed.finish("split", Some(base))?)?;
    assert_ne!(view.stat("a")?.0, view.stat("b")?.0);
    assert_eq!(view.read_range("b", 0, 100)?, b"original");
    assert_eq!(view.read_range("a", 0, 100)?, b"replacement");
    Ok(())
}

#[test]
fn conventional_oci_roundtrip_excludes_named_results() -> Result<(), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("file"), "filesystem")?;
    let results = BTreeMap::from([("result".into(), Value::from("native only"))]);
    let (edit, _) = capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &results,
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let mut archive = Vec::new();
    crate::filesystem::export(
        &edit.store,
        edit.finish("oci", None)?,
        scratch.path(),
        &mut archive,
    )?;
    let (store, revision, _, format) = crate::native::import_with_format(
        archive.as_slice(),
        scratch.path(),
        WorkspaceLimits::default(),
    )?;
    assert_eq!(format, "oci");
    let view = View::new(store, revision)?;
    assert_eq!(view.read_range("file", 0, 100)?, b"filesystem");
    assert!(read_results(&view)?.is_empty());
    Ok(())
}
