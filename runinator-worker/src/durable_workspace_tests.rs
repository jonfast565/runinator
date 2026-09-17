//! Durable workspace lifecycle phase reporting.
use super::*;
use runinator_workspace::storage::{
    self,
    staging::{EmptyStore, Staging},
    store::{ReadStore, WriteStore},
};
use std::{collections::HashMap, sync::Arc};

fn workspace_http_error(message: &str) -> ApiError {
    ApiError::Http {
        status: reqwest::StatusCode::CONFLICT,
        url: reqwest::Url::parse("http://runinator.test/workspaces/checkouts/test/content")
            .unwrap(),
        message: message.into(),
    }
}

#[test]
fn completed_phases_are_bounded_summary_records() {
    let reporter = WorkspacePhaseReporter::default();
    reporter
        .start("workspace.restore.index")
        .succeeded(runinator_models::json!({"objects": 52_000, "packs": 1}));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].phase, "workspace.restore.index");
    assert_eq!(events[0].status, "succeeded");
    assert_eq!(events[0].details["objects"], 52_000);
    assert!(reporter.drain().is_empty());
}

#[test]
fn an_unfinished_phase_records_one_failure() {
    let reporter = WorkspacePhaseReporter::default();
    drop(reporter.start("workspace.snapshot.capture"));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status, "failed");
}

#[tokio::test]
async fn restore_retries_until_the_workspace_claim_is_visible() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let bytes = retry_pending_workspace_claim(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        |_| {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(if call == 0 {
                Err(workspace_http_error(
                    "workspace.conflict: WORKSPACE002 - Workspace version or checkout is no longer current: replica has not claimed this active attempt",
                ))
            } else {
                Ok(vec![1, 2, 3])
            })
        },
    )
    .await
    .unwrap();

    assert_eq!(bytes, vec![1, 2, 3]);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn restore_does_not_retry_a_genuinely_stale_checkout() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let error = retry_pending_workspace_claim(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        |_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Err(workspace_http_error(
                "workspace.conflict: WORKSPACE002 - Workspace version or checkout is no longer current: checkout is no longer active",
            )))
        },
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("checkout is no longer active"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn injected_checkout_transport_preserves_claim_retry_and_scope() {
    let source = CheckoutSource {
        checkout: uuid::Uuid::new_v4(),
        replica: uuid::Uuid::new_v4(),
        calls: Default::default(),
    };
    let bytes = download_workspace_checkout_after_claim(
        &source,
        source.checkout,
        source.replica,
        std::time::Instant::now() + std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(bytes, vec![7, 8]);
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

fn workspace_fixture(files: usize) -> Result<(WorkspaceApi, WorkspaceExecution), SendableError> {
    let source = tempfile::tempdir()?;
    let scratch = tempfile::tempdir()?;
    std::fs::create_dir(source.path().join("files"))?;
    for index in 0..files {
        std::fs::write(
            source.path().join("files").join(format!("{index:04}")),
            format!("base content {index}"),
        )?;
    }
    let results = std::collections::BTreeMap::from([
        ("result".into(), Value::from("old")),
        ("retained".into(), Value::from(42)),
    ]);
    let (edit, usage) = runinator_workspace::revision::capture(
        Staging::new(EmptyStore, scratch.path())?,
        source.path(),
        &results,
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let revision = edit.finish("base", None)?;
    let mut archive = Vec::new();
    runinator_workspace::native::export(&edit.store, revision, scratch.path(), &mut archive)?;
    let marks = storage::gc::mark_roots(&edit.store, &[revision], scratch.path(), false)?;
    let mut objects = HashMap::new();
    marks.visit(|id| {
        let object = edit.store.get(id)?;
        let mut bytes = Vec::new();
        storage::record::write(&mut bytes, object.kind, &object.bytes)?;
        objects.insert(id.to_string(), bytes);
        Ok(())
    })?;

    let workspace_id = uuid::Uuid::new_v4();
    let workflow_run_id = uuid::Uuid::new_v4();
    let effect_id = uuid::Uuid::new_v4();
    let checkout = WorkspaceCheckout {
        limits: WorkspaceLimits::default(),
        id: uuid::Uuid::new_v4(),
        workspace_id,
        workflow_run_id,
        effect_id,
        attempt: 1,
        base_version: 1,
        access: WorkspaceAccess::Write,
        fence: 1,
        leased_until: chrono::Utc::now() + chrono::Duration::minutes(5),
    };
    let snapshot = WorkspaceSnapshot {
        workspace_id,
        version: 1,
        parent_version: 0,
        origin: WorkspaceOrigin::Workflow {
            workflow_run_id,
            effect_id,
            attempt: 1,
        },
        revision_id: revision.to_string(),
        usage,
        limits: WorkspaceLimits::default(),
        created_at: chrono::Utc::now(),
    };
    let api = WorkspaceApi {
        checkout: checkout.clone(),
        archive: Arc::new(archive),
        objects: Arc::new(objects),
        downloads: Default::default(),
        reads: Default::default(),
        uploads: Default::default(),
    };
    Ok((
        api,
        WorkspaceExecution {
            key: "test".into(),
            checkout,
            snapshot: Some(snapshot),
            results: Default::default(),
        },
    ))
}

#[tokio::test]
async fn result_only_checkpoint_seals_a_bounded_delta_without_restoring_files()
-> Result<(), SendableError> {
    let (api, execution) = workspace_fixture(128)?;
    let root = tempfile::tempdir()?;
    let workspace = ActiveWorkspace::restore_in(
        &api,
        &Value::encode(&execution)?,
        uuid::Uuid::new_v4(),
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        WorkspacePhaseReporter::default(),
        WorkspaceRestoreOptions {
            root: root.path().to_owned(),
            materialize_files: false,
            load_results: false,
        },
    )
    .await?;

    assert_eq!(api.downloads.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(workspace.path.read_dir()?.next().is_none());
    assert!(workspace.results().is_empty());
    assert!(workspace.drain_phases().is_empty());

    let output = Value::from("new");
    let commit = workspace.save(&api, Some(&output), true).await?.unwrap();
    let reads = api.reads.lock().unwrap();
    assert!(
        reads.len() < 64,
        "result-only checkpoint fetched {} objects for 128 files",
        reads.len()
    );
    assert_eq!(commit.snapshot.parent_version, 1);
    let uploads = api.uploads.lock().unwrap().clone();
    assert!(!uploads.is_empty());

    let verify_scratch = tempfile::tempdir()?;
    let (base, _, _) = runinator_workspace::native::import_packed(
        api.archive.as_slice(),
        verify_scratch.path(),
        WorkspaceLimits::default(),
    )?;
    let delta_scratch = tempfile::tempdir()?;
    let delta = Staging::new_deduplicating(&base, delta_scratch.path())?;
    for bytes in uploads {
        let pack = tempfile::NamedTempFile::new_in(delta_scratch.path())?;
        std::fs::write(pack.path(), &bytes)?;
        storage::record::index_pack(pack.path(), storage::Id::sha256(&bytes), |location, _| {
            let object = storage::record::decode_range(
                &bytes[location.offset as usize..(location.offset + location.length) as usize],
                location.id,
                location.member,
            )?;
            assert_eq!(delta.put(object.kind, &object.bytes)?, location.id);
            Ok(())
        })?;
    }
    let view = storage::view::View::new(&delta, commit.snapshot.revision_id.parse()?)?;
    assert_eq!(
        runinator_workspace::revision::read_results(&view)?,
        std::collections::BTreeMap::from([
            ("result".into(), Value::from("new")),
            ("retained".into(), Value::from(42)),
        ])
    );
    Ok(())
}

#[tokio::test]
async fn ordinary_checkpoint_restores_one_archive_and_never_uses_object_http()
-> Result<(), SendableError> {
    let (api, execution) = workspace_fixture(128)?;
    let root = tempfile::tempdir()?;
    let workspace = ActiveWorkspace::restore_in(
        &api,
        &Value::encode(&execution)?,
        uuid::Uuid::new_v4(),
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        WorkspacePhaseReporter::default(),
        WorkspaceRestoreOptions {
            root: root.path().to_owned(),
            materialize_files: true,
            load_results: true,
        },
    )
    .await?;

    assert_eq!(api.downloads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(workspace.path.join("files/0127"))?,
        b"base content 127"
    );
    assert_eq!(workspace.results()["retained"], Value::from(42));
    assert!(api.reads.lock().unwrap().is_empty());
    let phases = workspace.drain_phases();
    assert!(
        phases
            .iter()
            .any(|phase| phase.phase == "workspace.restore.download")
    );
    assert!(
        phases
            .iter()
            .any(|phase| phase.phase == "workspace.restore.materialize")
    );

    workspace
        .save(&api, Some(&Value::from("updated")), false)
        .await?;
    assert!(api.reads.lock().unwrap().is_empty());
    assert!(!api.uploads.lock().unwrap().is_empty());
    Ok(())
}

#[path = "durable_workspace_tests/checkout_source.rs"]
mod checkout_source;
use checkout_source::CheckoutSource;

#[path = "durable_workspace_tests/workspace_api.rs"]
mod workspace_api;
use workspace_api::WorkspaceApi;
