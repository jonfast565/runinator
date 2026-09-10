//! Cached base membership for outgoing workspace packs.
use super::*;

#[test]
fn cached_base_omits_only_verified_remote_objects() -> storage::Result<()> {
    use storage::{Layout, packs, staging::Staging, transaction::Edit};

    let cache = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    let base = CachedObjects {
        path: cache.path(),
        local: None,
    };
    let stage = Staging::new(&base, scratch.path())?;
    let mut edit = Edit::new(stage, None, Layout::default(), 8 * 1024 * 1024)?;
    edit.put("repository", b"cloned content".as_slice())?;
    let revision = edit.finish("clone", None)?;
    let object = edit.store.get(revision)?;
    assert!(!base.contains(revision)?);

    let mut file = fs::File::create(cache.path().join(revision.to_string()))?;
    storage::record::write(&mut file, object.kind, &object.bytes)?;
    assert!(base.contains(revision)?);

    let mut count = 0;
    packs::seal(&edit.store, &base, revision, scratch.path(), |pack| {
        for n in 0..pack.index.count {
            assert_ne!(pack.index.entry(n)?.id, revision);
            count += 1;
        }
        Ok(())
    })?;
    assert!(count > 0);
    fs::write(cache.path().join(revision.to_string()), b"corrupt")?;
    assert!(base.contains(revision).is_err());
    Ok(())
}

#[test]
fn downloaded_base_is_used_without_a_remote_object_cache_entry() -> storage::Result<()> {
    use storage::{model::Kind, staging::Staging, store::WriteStore};

    let cache = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    let store = Staging::new(storage::staging::EmptyStore, scratch.path())?;
    let id = store.put(Kind::Chunk, b"downloaded base object")?;
    let local = LocalObjects::new(store, scratch);
    let base = CachedObjects {
        path: cache.path(),
        local: Some(&local),
    };

    assert_eq!(&*base.get(id)?.bytes, b"downloaded base object");
    assert!(!cache.path().join(id.to_string()).exists());
    Ok(())
}
