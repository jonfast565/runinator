//! Cached base membership for outgoing workspace packs.
use super::*;
use runinator_api::{AsyncApiClient, StaticLocator};

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
fn downloaded_base_is_used_without_a_remote_object_cache_entry()
-> Result<(), runinator_models::errors::SendableError> {
    use storage::staging::{EmptyStore, Staging};

    let cache = tempfile::tempdir()?;
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("file"), b"downloaded base object")?;
    let (edit, _) = runinator_workspace::revision::capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &Default::default(),
        Default::default(),
        scratch.path(),
    )?;
    let id = edit.finish("test", None)?;
    let mut archive = Vec::new();
    runinator_workspace::native::export(&edit.store, id, scratch.path(), &mut archive)?;
    let import_scratch = tempfile::tempdir()?;
    let (store, imported, _) = runinator_workspace::native::import_packed(
        archive.as_slice(),
        import_scratch.path(),
        Default::default(),
    )?;
    assert_eq!(imported, id);
    let local = LocalObjects::new(store, import_scratch);
    let base = CachedObjects {
        path: cache.path(),
        local: Some(&local),
    };

    assert_eq!(base.get(id)?.kind, storage::model::Kind::Revision);
    assert!(!cache.path().join(id.to_string()).exists());
    Ok(())
}

#[tokio::test]
async fn downloaded_base_miss_never_falls_back_to_the_object_endpoint()
-> Result<(), runinator_models::errors::SendableError> {
    use storage::{
        model::Kind,
        staging::{EmptyStore, Staging},
    };

    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    fs::write(source.path().join("file"), b"complete downloaded base")?;
    let (edit, _) = runinator_workspace::revision::capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &Default::default(),
        Default::default(),
        scratch.path(),
    )?;
    let revision = edit.finish("test", None)?;
    let mut archive = Vec::new();
    runinator_workspace::native::export(&edit.store, revision, scratch.path(), &mut archive)?;
    let import_scratch = tempfile::tempdir()?;
    let (store, imported, _) = runinator_workspace::native::import_packed(
        archive.as_slice(),
        import_scratch.path(),
        Default::default(),
    )?;
    assert_eq!(imported, revision);

    let objects = WorkerObjects::new(
        AsyncApiClient::new(StaticLocator::new("http://127.0.0.1:1"))?,
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        Instant::now() + std::time::Duration::from_secs(5),
        Some(LocalObjects::new(store, import_scratch)),
    )?;
    let missing = storage::Id::object(Kind::Revision, b"not in the complete archive");
    assert!(matches!(
        objects.info(missing),
        Err(storage::Error::NotFound(_))
    ));
    assert!(matches!(
        objects.get(missing),
        Err(storage::Error::NotFound(_))
    ));
    Ok(())
}

struct ObjectTransport {
    checkout: uuid::Uuid,
    replica: uuid::Uuid,
    uploads: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}
#[async_trait::async_trait]
impl WorkspaceObjectTransport for ObjectTransport {
    async fn workspace_object(
        &self,
        checkout: uuid::Uuid,
        replica: uuid::Uuid,
        _: &str,
    ) -> runinator_api::Result<Option<Vec<u8>>> {
        assert_eq!((checkout, replica), (self.checkout, self.replica));
        Ok(None)
    }
    async fn upload_workspace_pack(
        &self,
        checkout: uuid::Uuid,
        replica: uuid::Uuid,
        bytes: Vec<u8>,
    ) -> runinator_api::Result<()> {
        assert_eq!((checkout, replica), (self.checkout, self.replica));
        self.uploads.lock().unwrap().push(bytes);
        Ok(())
    }
}

#[tokio::test]
async fn injected_object_transport_keeps_checkout_scope_and_missing_objects() {
    let checkout = uuid::Uuid::new_v4();
    let replica = uuid::Uuid::new_v4();
    let uploads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let objects = WorkerObjects::new(
        ObjectTransport {
            checkout,
            replica,
            uploads: uploads.clone(),
        },
        checkout,
        replica,
        Instant::now() + std::time::Duration::from_secs(1),
        None,
    )
    .unwrap();
    tokio::task::spawn_blocking(move || {
        objects.upload(vec![1, 2, 3]).unwrap();
        let id: Id = "00".repeat(32).parse().unwrap();
        assert!(matches!(objects.get(id), Err(storage::Error::NotFound(_))));
    })
    .await
    .unwrap();
    assert_eq!(*uploads.lock().unwrap(), vec![vec![1, 2, 3]]);
}
