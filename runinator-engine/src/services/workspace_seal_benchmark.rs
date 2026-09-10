//! Compare complete seal validation over identical registered packs using both storage paths.
use super::*;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    auth::ResourceType,
    rbac::{ResourceOwnership, ScopeRef},
    workspaces::*,
};
use runinator_store::{DatabaseImpl, roles::DurableWorkspaceStore};
use std::sync::Arc;

struct ValidationTimings {
    graph: std::time::Duration,
    view: std::time::Duration,
    usage: std::time::Duration,
    results: std::time::Duration,
    links: std::time::Duration,
    total: std::time::Duration,
}

fn validate<S: runinator_workspace::storage::store::ReadStore>(
    store: S,
    revision: runinator_workspace::storage::Id,
    verified_info: bool,
) -> (WorkspaceUsage, ValidationTimings) {
    use runinator_workspace::{
        revision,
        storage::{gc, view::View},
    };
    let scratch = tempfile::tempdir().unwrap();
    let total = std::time::Instant::now();
    let phase = std::time::Instant::now();
    if verified_info {
        gc::verify_roots_with_verified_info(&store, &[revision], scratch.path(), false).unwrap();
    } else {
        gc::verify_roots(&store, &[revision], scratch.path(), false).unwrap();
    }
    let graph = phase.elapsed();
    let phase = std::time::Instant::now();
    let view = View::new(store, revision).unwrap();
    let view_time = phase.elapsed();
    let phase = std::time::Instant::now();
    let usage = revision::usage(&view).unwrap();
    let usage_time = phase.elapsed();
    let phase = std::time::Instant::now();
    revision::validate_results(&view).unwrap();
    let results = phase.elapsed();
    let phase = std::time::Instant::now();
    revision::validate_links(&view).unwrap();
    let links = phase.elapsed();
    (
        usage,
        ValidationTimings {
            graph,
            view: view_time,
            usage: usage_time,
            results,
            links,
            total: total.elapsed(),
        },
    )
}

#[tokio::test]
#[ignore = "manual seal performance comparison"]
async fn compare_seal_validation_backends() {
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
    let limits = WorkspaceLimits::default();
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
        let mut seed = 7u64;
        let mut content = vec![0u8; 32 * 1024 * 1024];
        for byte in &mut content {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }
        std::fs::write(source.path().join("large"), &content).unwrap();
        for i in 0..1000 {
            std::fs::write(
                source.path().join(format!("file-{i}")),
                &content[i * 4096..(i + 1) * 4096],
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
    assert!(
        service
            .upload_pack(checkout.id, b"bad pack".to_vec())
            .await
            .is_err()
    );
    for pack in packs {
        service.upload_pack(checkout.id, pack).await.unwrap();
    }
    let old = service.objects(id);
    let optimized = super::workspace_seal_objects::SealObjects::new(service.objects(id).inner);
    tokio::task::spawn_blocking(move || {
        let revision = revision_id.parse().unwrap();
        let start = std::time::Instant::now();
        let (before, old_phases) = validate(old, revision, false);
        let old_time = start.elapsed();
        let start = std::time::Instant::now();
        let (after, optimized_phases) = validate(&optimized, revision, true);
        let new_time = start.elapsed();
        let (queries, blob_reads) = optimized.io_counts();
        assert!(queries < 100);
        assert!(blob_reads <= 2);
        assert_eq!(before, after);
        println!("seal benchmark: entries={} logical_bytes={} previous={old_time:?} optimized={new_time:?}", after.entries, after.logical_bytes);
        println!("previous phases: graph={:?} view={:?} usage={:?} results={:?} links={:?} total={:?}", old_phases.graph, old_phases.view, old_phases.usage, old_phases.results, old_phases.links, old_phases.total);
        println!("optimized phases: graph={:?} view={:?} usage={:?} results={:?} links={:?} total={:?}", optimized_phases.graph, optimized_phases.view, optimized_phases.usage, optimized_phases.results, optimized_phases.links, optimized_phases.total);
        println!("optimized database_queries={queries} blob_reads={blob_reads}");
    }).await.unwrap();
}
