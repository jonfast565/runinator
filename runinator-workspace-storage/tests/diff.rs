use runinator_workspace_storage::{
    Layout, diff, store::MemoryStore, transaction::Edit, view::View,
};

#[test]
fn resumes_filesystem_and_result_changes_with_revision_binding()
-> runinator_workspace_storage::Result<()> {
    let mut edit = Edit::new(
        MemoryStore::default(),
        None,
        Layout::default(),
        8 * 1024 * 1024,
    )?;
    edit.mkdir("a")?;
    edit.put("a/old", b"old".as_slice())?;
    edit.put("unchanged", b"same".as_slice())?;
    let before = edit.finish("before", None)?;
    edit.unlink("a/old")?;
    edit.put("a/new", b"new".as_slice())?;
    let result = runinator_workspace_storage::pages::ingest(
        &edit.store,
        Layout::default(),
        b"42".as_slice(),
    )?;
    edit.attachments =
        runinator_workspace_storage::radix::set(&edit.store, None, b"answer", Some(result))?;
    let after = edit.finish("after", Some(before))?;
    let a = View::new(&edit.store, before)?;
    let b = View::new(&edit.store, after)?;
    let first = diff::page(&a, &b, None, 1)?;
    assert!(diff::page(&b, &a, first.cursor.clone(), 1).is_err());
    let mut changes = first.changes;
    let mut cursor = first.cursor;
    while let Some(next) = cursor {
        let wire = serde_json::to_string(&next)?;
        assert!(wire.len() < 1024);
        let page = diff::page(&a, &b, Some(serde_json::from_str(&wire)?), 1)?;
        changes.extend(page.changes);
        cursor = page.cursor;
    }
    assert_eq!(
        changes
            .iter()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        ["a/new", "a/old", "answer"]
    );
    assert!(changes[2].result);
    assert!(diff::page(&b, &b, None, 200)?.changes.is_empty());
    Ok(())
}
