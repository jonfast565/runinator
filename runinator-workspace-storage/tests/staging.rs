use runinator_workspace_storage::{
    Error, Id, Result,
    model::Kind,
    staging::Staging,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
};

struct UnavailableBase;

impl ReadStore for UnavailableBase {
    fn info(&self, _: Id) -> Result<ObjectInfo> {
        Err(Error::Busy("remote storage unavailable".into()))
    }
    fn get(&self, _: Id) -> Result<Object> {
        Err(Error::Busy("remote storage unavailable".into()))
    }
}

#[test]
fn staging_writes_and_duplicate_writes_do_not_read_remote_storage() -> Result<()> {
    let scratch = tempfile::tempdir()?;
    let store = Staging::new(UnavailableBase, scratch.path())?;
    let bytes = b"new repository content";
    let id = store.put(Kind::Chunk, bytes)?;
    assert_eq!(store.put(Kind::Chunk, bytes)?, id);
    assert_eq!(store.get(id)?.bytes.as_slice(), bytes);
    assert!(store.get(Id::default()).is_err());
    Ok(())
}
