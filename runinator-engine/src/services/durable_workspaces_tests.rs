//! Shared pack storage, validation, and receipt issuance.
use super::*;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    auth::ResourceType,
    rbac::{ResourceOwnership, ScopeRef},
    workspaces::*,
};
use runinator_store::roles::durable_workspaces::WorkspaceRetentionStore;
use runinator_store::{DatabaseImpl, roles::DurableWorkspaceStore};
use std::sync::Arc;

#[tokio::test]
async fn packs_are_validated_before_receipt_and_receipts_do_not_publish() {
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
    let service = WorkspaceService::new(db.clone(), blobs);
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
        max_bytes: 1024,
        max_entries: 10,
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
        std::fs::write(source.path().join("file"), b"workspace data").unwrap();
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
    assert!(
        service
            .upload_pack(checkout.id, b"bad pack".to_vec())
            .await
            .is_err()
    );
    for pack in packs {
        service.upload_pack(checkout.id, pack).await.unwrap();
    }
    let objects = service.objects(id);
    let verified_revision = revision_id.parse().unwrap();
    tokio::task::spawn_blocking(move || {
        use runinator_workspace::storage::{gc, store::ReadStore};
        let scratch = tempfile::tempdir().unwrap();
        // bypass the logical cache to exercise shared physical records across verification passes.
        gc::verify_roots(&objects.inner, &[verified_revision], scratch.path(), false).unwrap();
        let first = objects.inner.records.stats().unwrap();
        objects.inner.get(verified_revision).unwrap();
        let second = objects.inner.records.stats().unwrap();
        assert_eq!(second.misses, first.misses);
        assert!(second.hits > first.hits);
        assert!(second.resident_bytes <= objects.inner.records.capacity());
    })
    .await
    .unwrap();
    let receipt = service
        .seal(
            checkout.id,
            WorkspaceSeal {
                revision_id: revision_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.checkout, checkout);
    assert_eq!(receipt.snapshot.revision_id, revision_id);
    assert_eq!(receipt.snapshot.usage.logical_bytes, 16);
    assert_eq!(receipt.snapshot.limits, limits);
    assert!(service.versions(id, 10, 0).await.unwrap().is_empty());
    assert_eq!(service.get(id).await.unwrap().unwrap().head_version, 0);
    db.release_workspace_checkout(checkout.id, checkout.fence)
        .await
        .unwrap();
    assert!(
        service
            .seal(checkout.id, WorkspaceSeal { revision_id })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_import_export_jobs_publish_once_and_stream_across_service_restart() {
    use runinator_blob_core::BlobStore;
    use tokio::io::AsyncReadExt;
    let temp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDb::new(temp.path().join("jobs.sqlite").to_str().unwrap())
            .await
            .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let blobs = Arc::new(
        runinator_blob_core::FsBlobStore::open(temp.path().join("blobs"))
            .await
            .unwrap(),
    );
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
                key: "import".into(),
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
    let archive = tokio::task::spawn_blocking(|| {
        use runinator_workspace::{
            native, revision,
            storage::staging::{EmptyStore, Staging},
        };
        let source = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("file"), "native import data").unwrap();
        let results = std::collections::BTreeMap::from([(
            "result".into(),
            runinator_models::value::Value::from("answer"),
        )]);
        let stage = Staging::new(EmptyStore, scratch.path()).unwrap();
        let (edit, _) = revision::capture(
            stage,
            source.path(),
            &results,
            WorkspaceLimits::default(),
            scratch.path(),
        )
        .unwrap();
        let revision = edit.finish("import", None).unwrap();
        let mut archive = Vec::new();
        native::export(&edit.store, revision, scratch.path(), &mut archive).unwrap();
        archive
    })
    .await
    .unwrap();
    let job = service.create_transfer(id, 0, true, false).await.unwrap();
    assert!(service.create_transfer(id, 0, true, false).await.is_err());
    service
        .upload_transfer(job.id, archive.as_slice())
        .await
        .unwrap();
    assert_eq!(service.get(id).await.unwrap().unwrap().head_version, 0);
    drop(service);
    let restarted = WorkspaceService::new(db.clone(), blobs);
    restarted.run_transfers().await.unwrap();
    let done = restarted.transfer(job.id).await.unwrap();
    assert_eq!(done.state, "ready", "{:?}", done.error);
    assert_eq!(restarted.get(id).await.unwrap().unwrap().head_version, 1);
    assert_eq!(
        restarted.snapshot(id, 1).await.unwrap().origin,
        WorkspaceOrigin::Import {
            transfer_id: job.id,
            format: "native".into()
        }
    );
    assert_eq!(
        restarted
            .file_range(id, 1, "file".into(), 7, 6)
            .await
            .unwrap(),
        b"import"
    );
    let results = restarted.results(id, 1, None, 10).await.unwrap();
    assert_eq!(results.entries[0].name, "result");
    let export = restarted
        .create_transfer(id, 1, false, false)
        .await
        .unwrap();
    restarted.run_transfers().await.unwrap();
    let mut content = restarted.transfer_content(export.id).await.unwrap();
    let mut downloaded = Vec::new();
    content.body.read_to_end(&mut downloaded).await.unwrap();
    tokio::task::spawn_blocking(move || {
        let scratch = tempfile::tempdir().unwrap();
        let (objects, revision, _) = runinator_workspace::native::import(
            downloaded.as_slice(),
            scratch.path(),
            WorkspaceLimits::default(),
        )
        .unwrap();
        let view = runinator_workspace::storage::view::View::new(objects, revision).unwrap();
        assert_eq!(
            view.read_range("file", 0, 100).unwrap(),
            b"native import data"
        );
        assert_eq!(
            runinator_workspace::revision::read_result(&view, "result").unwrap(),
            runinator_models::value::Value::from("answer")
        );
    })
    .await
    .unwrap();
    // an abandoned writer adds unreachable data and forces a physical repack.
    let acquired = db
        .acquire_workspace_checkout(WorkspaceAcquire {
            workspace_id: id,
            workflow_run_id: uuid::Uuid::now_v7(),
            effect_id: uuid::Uuid::now_v7(),
            attempt: 0,
            version: Some(1),
            access: WorkspaceAccess::Write,
            limits: WorkspaceLimits::default(),
            now: chrono::Utc::now(),
            leased_until: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .unwrap();
    let WorkspaceAcquisition::Acquired { checkout } = acquired else {
        panic!("writer");
    };
    let mut pack = runinator_workspace::storage::record::PACK_MAGIC.to_vec();
    runinator_workspace::storage::record::write(
        &mut pack,
        runinator_workspace::storage::model::Kind::Chunk,
        b"abandoned bytes",
    )
    .unwrap();
    restarted.upload_pack(checkout.id, pack).await.unwrap();
    db.release_workspace_checkout(checkout.id, checkout.fence)
        .await
        .unwrap();
    let reader = db.pin_workspace_reader(id, 1).await.unwrap();
    restarted.collect_workspace(id).await.unwrap();
    assert!(db.expired_workspace_packs(id).await.unwrap().is_empty());
    db.release_workspace_reader(reader.id).await.unwrap();
    assert!(!db.expired_workspace_packs(id).await.unwrap().is_empty());
    restarted.collect_workspace(id).await.unwrap();
    assert_eq!(
        restarted
            .file_range(id, 1, "file".into(), 0, 100)
            .await
            .unwrap(),
        b"native import data"
    );
}
