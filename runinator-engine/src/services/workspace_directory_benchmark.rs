//! Repeatable sequential/bulk directory timings against real SQLite and blob storage.
use super::*;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    auth::ResourceType,
    rbac::{ResourceOwnership, ScopeRef},
    workspaces::*,
};
use runinator_store::{DatabaseImpl, roles::DurableWorkspaceStore};
use runinator_workspace::storage::{
    Id, Result,
    store::{Object, ObjectInfo, ReadStore},
    view::View,
};
use std::sync::{Arc, atomic::Ordering};
struct Sequential<S>(S);
impl<S: ReadStore> ReadStore for Sequential<S> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.0.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.0.get(id)
    }
}
#[tokio::test]
#[ignore = "directory performance fixture; run explicitly with --ignored --nocapture"]
async fn sequential_and_bulk_directory_measurements() {
    let temp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDb::new(temp.path().join("database.sqlite").to_str().unwrap())
            .await
            .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let blobs = Arc::new(
        runinator_blob_core::FsBlobStore::open(temp.path().join("blobs"))
            .await
            .unwrap(),
    );
    use runinator_blob_core::BlobStore;
    blobs
        .create_bucket(runinator_blob_core::WORKSPACE_BUCKET)
        .await
        .unwrap();
    let service = WorkspaceService::new(db.clone(), blobs.clone());
    let now = chrono::Utc::now();
    let id = uuid::Uuid::now_v7();
    service
        .create(
            DurableWorkspace {
                id,
                key: "native".into(),
                org_id: None,
                head_version: 0,
                revision: 1,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            },
            ResourceOwnership {
                resource_type: ResourceType::Workspace,
                resource_id: id,
                tenant: ScopeRef::PLATFORM,
                owner: ScopeRef::PLATFORM,
                created_by: None,
                authz_version: 1,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
    let limits = WorkspaceLimits {
        max_bytes: 1024 * 1024,
        max_entries: 2000,
        max_results_bytes: 512,
    };
    let acquired = db
        .acquire_workspace_checkout(WorkspaceAcquire {
            limits,
            workspace_id: id,
            workflow_run_id: uuid::Uuid::now_v7(),
            effect_id: uuid::Uuid::now_v7(),
            attempt: 0,
            version: None,
            access: WorkspaceAccess::Write,
            now,
            leased_until: now + chrono::Duration::minutes(5),
        })
        .await
        .unwrap();
    let WorkspaceAcquisition::Acquired { checkout } = acquired else {
        panic!("acquire");
    };
    let (revision_id, packs) = tokio::task::spawn_blocking(|| {
        use runinator_workspace::storage::{
            packs,
            staging::{EmptyStore, Staging},
        };
        let source = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        for n in 0..1001 {
            std::fs::write(
                source.path().join(format!("file-{n:05}")),
                format!("workspace data {n}"),
            )
            .unwrap();
        }
        let store = Staging::new(EmptyStore, scratch.path()).unwrap();
        let (edit, _) = runinator_workspace::revision::capture(
            store,
            source.path(),
            &Default::default(),
            WorkspaceLimits::default(),
            scratch.path(),
        )
        .unwrap();
        let revision = edit.finish("native", None).unwrap();
        let mut output = Vec::new();
        packs::seal(&edit.store, &EmptyStore, revision, scratch.path(), |pack| {
            output.push(std::fs::read(pack.path)?);
            Ok(())
        })
        .unwrap();
        (revision.to_string(), output)
    })
    .await
    .unwrap();
    for pack in packs {
        service.upload_pack(checkout.id, pack).await.unwrap();
    }
    for sequential in [true, false] {
        let reader = WorkspaceService::new(db.clone(), blobs.clone());
        let store = Sequential(reader.objects(id));
        let warm_store = Sequential(reader.objects(id));
        let revision: Id = revision_id.parse().unwrap();
        tokio::task::spawn_blocking(move || {
            let started = std::time::Instant::now();
            let reader: &dyn ReadStore = if sequential { &store } else { &store.0 };
            let view = View::new(reader, revision).unwrap();
            let mut after = None;
            let mut count = 0;
            let mut first_ms = 0;
            loop {
                let page = view.directory("", after.as_deref(), 200).unwrap();
                if after.is_none() { first_ms = started.elapsed().as_millis(); }
                count += page.len();
                if page.len() < 200 { break; }
                after = page.last().map(|entry| entry.name.clone());
            }
            assert_eq!(count, 1001);
            let database_reads = store.0.inner.database_reads.load(Ordering::Relaxed);
            let blob_reads = store.0.inner.blob_reads.load(Ordering::Relaxed);
            if !sequential {
                assert!(database_reads < 100, "bulk directory used {database_reads} database reads");
                assert!(blob_reads < 100, "bulk directory used {blob_reads} blob reads");
            }
            println!("sequential={sequential} entries={count} first_ms={first_ms} total_ms={} database_reads={database_reads} blob_reads={blob_reads}", started.elapsed().as_millis());
            if !sequential {
                let warm_started = std::time::Instant::now();
                let warm_view = View::new(&warm_store.0, revision).unwrap();
                assert_eq!(warm_view.directory("", None, 200).unwrap().len(), 200);
                println!("warm_first_ms={} database_reads={} blob_reads={}",
                    warm_started.elapsed().as_millis(),
                    warm_store.0.inner.database_reads.load(Ordering::Relaxed),
                    warm_store.0.inner.blob_reads.load(Ordering::Relaxed));
            }
        }).await.unwrap();
    }
}
