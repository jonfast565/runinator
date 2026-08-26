//! append-only workflow revision history: sequence assignment, unchanged-save dedupe, lookup by
//! sequence number, and removal with the workflow that owns it.

use super::*;

fn revision_for(saved: &WorkflowDefinition, source: RevisionSource) -> WorkflowRevision {
    WorkflowRevision {
        id: Uuid::nil(),
        workflow_id: saved.id.unwrap(),
        revision: 0,
        digest: WorkflowRevision::content_digest(
            saved.version,
            &saved.input_type,
            &saved.definition,
        ),
        version: saved.version,
        name: saved.name.clone(),
        input_type: saved.input_type.clone(),
        definition: saved.definition.clone(),
        source,
        actor_id: None,
        actor_kind: "system".to_string(),
        note: None,
        created_at: None,
    }
}

async fn open(label: &str) -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-revisions-{label}-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

#[tokio::test]
async fn revisions_number_from_one_and_increment_per_change() {
    let (db, path) = open("sequence").await;
    let saved = db.upsert_workflow(&workflow("revised")).await.unwrap();

    let first = db
        .insert_workflow_revision(&revision_for(&saved, RevisionSource::Ui))
        .await
        .unwrap()
        .expect("first revision recorded");
    assert_eq!(first.revision, 1);

    let mut changed = saved.clone();
    changed.definition = WorkflowGraph::from_value(
        runinator_models::json!({ "nodes": [{ "id": "done", "kind": "end" }] }),
    )
    .unwrap();
    let second = db
        .insert_workflow_revision(&revision_for(&changed, RevisionSource::Pack))
        .await
        .unwrap()
        .expect("changed definition recorded");
    assert_eq!(second.revision, 2);
    assert_eq!(second.source, RevisionSource::Pack);

    // newest first, so the list reads as history.
    let listed = db
        .fetch_workflow_revisions(saved.id.unwrap(), 50)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|r| r.revision).collect::<Vec<_>>(),
        vec![2, 1]
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn an_unchanged_save_records_no_revision() {
    let (db, path) = open("dedupe").await;
    let saved = db.upsert_workflow(&workflow("stable")).await.unwrap();
    let revision = revision_for(&saved, RevisionSource::Pack);

    assert!(
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .is_some()
    );
    // a pack that reapplies on a cron must not bury real edits under identical rows.
    assert!(
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .is_none()
    );

    let listed = db
        .fetch_workflow_revisions(saved.id.unwrap(), 50)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);

    // a rename with an untouched graph is still a change worth recording.
    let mut renamed = saved.clone();
    renamed.name = "stable-renamed".to_string();
    let recorded = db
        .insert_workflow_revision(&revision_for(&renamed, RevisionSource::Ui))
        .await
        .unwrap()
        .expect("rename recorded");
    assert_eq!(recorded.revision, 2);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn a_revision_is_fetchable_by_its_sequence_number() {
    let (db, path) = open("lookup").await;
    let saved = db.upsert_workflow(&workflow("lookup")).await.unwrap();
    db.insert_workflow_revision(&revision_for(&saved, RevisionSource::Ui))
        .await
        .unwrap();

    let mut changed = saved.clone();
    changed.definition = WorkflowGraph::from_value(
        runinator_models::json!({ "nodes": [{ "id": "second", "kind": "end" }] }),
    )
    .unwrap();
    db.insert_workflow_revision(&revision_for(&changed, RevisionSource::Ui))
        .await
        .unwrap();

    let workflow_id = saved.id.unwrap();
    let first = db
        .fetch_workflow_revision(workflow_id, 1)
        .await
        .unwrap()
        .expect("revision 1");
    // the older definition survives intact — that is what makes rollback possible.
    assert_eq!(first.definition, saved.definition);
    assert_eq!(
        db.fetch_workflow_revision(workflow_id, 2)
            .await
            .unwrap()
            .unwrap()
            .definition,
        changed.definition
    );
    assert!(
        db.fetch_workflow_revision(workflow_id, 3)
            .await
            .unwrap()
            .is_none()
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn deleting_a_workflow_removes_its_revisions() {
    let (db, path) = open("cascade").await;
    let saved = db.upsert_workflow(&workflow("disposable")).await.unwrap();
    let workflow_id = saved.id.unwrap();
    db.insert_workflow_revision(&revision_for(&saved, RevisionSource::Ui))
        .await
        .unwrap();

    db.delete_workflow(workflow_id).await.unwrap();

    // sqlite only enforces the declared cascade with the pragma on, so this pins the explicit delete.
    assert!(
        db.fetch_workflow_revisions(workflow_id, 50)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_file(path);
}
