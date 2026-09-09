use runinator_workspace_storage::{
    Id, Layout, gc, packs, radix,
    staging::{EmptyStore, Staging},
    store::{MemoryStore, ReadStore},
    transaction::Edit,
    view::View,
};

#[test]
fn pages_resume_at_prefix_boundaries() -> runinator_workspace_storage::Result<()> {
    let store = MemoryStore::default();
    let mut root = None;
    let names = ["a", "aa", "ab", "abc", "b", "ba", "z"];
    for name in names {
        root = radix::set(&store, root, name.as_bytes(), Some(Id::default()))?;
    }
    for (i, name) in names.iter().enumerate() {
        let actual = radix::page(&store, root, Some(name.as_bytes()), 2)?;
        assert_eq!(
            actual
                .iter()
                .map(|(key, _)| std::str::from_utf8(key).unwrap())
                .collect::<Vec<_>>(),
            names[i + 1..].iter().take(2).copied().collect::<Vec<_>>()
        );
    }
    assert!(radix::page(&store, root, None, 1002).is_err());
    Ok(())
}

#[test]
fn selected_closure_omits_abandoned_edits_and_ancestors() -> runinator_workspace_storage::Result<()>
{
    let scratch = tempfile::tempdir()?;
    let store = Staging::new(EmptyStore, scratch.path())?;
    let mut edit = Edit::new(store, None, Layout::default(), 16 * 1024 * 1024)?;
    edit.put("old", b"old bytes".as_slice())?;
    let old = edit.finish("old", None)?;
    edit.unlink("old")?;
    let abandoned = edit.put("new", b"discarded".as_slice())?;
    edit.put("new", b"current".as_slice())?;
    let new = edit.finish("new", Some(old))?;
    let marks = gc::verify_roots(&edit.store, &[new], scratch.path(), false)?;
    assert!(!marks.contains(old)?);
    assert!(!marks.contains(abandoned)?);
    let mut ids = Vec::new();
    packs::seal(&edit.store, &EmptyStore, new, scratch.path(), |pack| {
        for n in 0..pack.index.count {
            let location = pack.index.entry(n)?;
            ids.push(location.id);
            let bytes = std::fs::read(&pack.path)?;
            let record =
                &bytes[location.offset as usize..(location.offset + location.length) as usize];
            let object = runinator_workspace_storage::record::decode_range(
                record,
                location.id,
                location.member,
            )?;
            assert_eq!(*object.bytes, *edit.store.get(location.id)?.bytes);
        }
        Ok(())
    })?;
    assert!(!ids.contains(&old));
    assert!(!ids.contains(&abandoned));
    let view = View::new(&edit.store, new)?;
    assert_eq!(view.directory("", None, 1)?[0].name, "new");
    assert_eq!(view.read_range("new", 1, 3)?, b"urr");
    Ok(())
}

#[test]
fn root_metadata_refreshes_both_roots() -> runinator_workspace_storage::Result<()> {
    let store = MemoryStore::default();
    let mut edit = Edit::new(store, None, Layout::default(), 8 * 1024 * 1024)?;
    edit.set_metadata(
        "",
        runinator_workspace_storage::Metadata {
            mode: 0o750,
            ..Default::default()
        },
    )?;
    let id = edit.finish("root", None)?;
    let temp = tempfile::tempdir()?;
    gc::verify_roots(&edit.store, &[id], temp.path(), false)?;
    Ok(())
}
