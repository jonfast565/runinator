//! workflow revision history end to end: what a save records, what a rollback does, and the two
//! things rollback deliberately refuses to do.

use super::*;

fn two_node_graph(second: &str) -> WorkflowGraph {
    WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": second } } },
            { "id": second, "kind": "end" }
        ]
    }))
    .unwrap()
}

#[tokio::test]
async fn each_accepted_definition_is_recorded_as_a_revision() {
    let (db, path) = test_db().await;

    let saved = save_workflow(&db, &workflow(None, "history"))
        .await
        .unwrap();
    let workflow_id = saved.id.unwrap();

    let mut edited = saved.clone();
    edited.definition = two_node_graph("finished");
    // compared against what the save *returned*: a definition is normalized on the way in, and the
    // revision captures the stored form rather than the submitted one.
    let edited = save_workflow(&db, &edited).await.unwrap();

    let revisions = crate::repository::fetch_workflow_revisions(&db, workflow_id, 50)
        .await
        .unwrap();
    assert_eq!(
        revisions.iter().map(|r| r.revision).collect::<Vec<_>>(),
        vec![2, 1]
    );
    // the older graph is still readable, which is the whole point of keeping it.
    assert_eq!(revisions[1].definition, saved.definition);
    assert_eq!(revisions[0].definition, edited.definition);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_pack_import_records_a_revision_attributed_to_the_pack() {
    let (db, path) = test_db().await;

    let bundle = WorkflowBundle {
        workflows: vec![workflow(None, "packaged")],
        triggers: Vec::new(),
    };
    let imported = crate::repository::import_workflow_bundle(&db, bundle)
        .await
        .unwrap();
    let workflow_id = imported.workflows[0].id.unwrap();

    let revisions = crate::repository::fetch_workflow_revisions(&db, workflow_id, 50)
        .await
        .unwrap();
    assert_eq!(revisions.len(), 1);
    // `workflows apply` overwrites wholesale, so knowing a change came from a pack is the
    // difference between a recoverable bad apply and a mystery.
    assert_eq!(revisions[0].source, RevisionSource::Pack);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reapplying_an_unchanged_pack_does_not_grow_history() {
    let (db, path) = test_db().await;

    let mut definition = workflow(None, "hourly");
    definition.updated_at = Some(chrono::Utc::now());
    for _ in 0..3 {
        crate::repository::import_workflow_bundle_with(
            &db,
            WorkflowBundle {
                workflows: vec![definition.clone()],
                triggers: Vec::new(),
            },
            true,
        )
        .await
        .unwrap();
    }

    let stored = crate::repository::fetch_workflow_by_name(&db, "hourly".into())
        .await
        .unwrap()
        .unwrap();
    let revisions = crate::repository::fetch_workflow_revisions(&db, stored.id.unwrap(), 50)
        .await
        .unwrap();
    // a pack on a cron would otherwise bury the edits worth seeing under identical rows.
    assert_eq!(revisions.len(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restoring_a_revision_writes_it_forward_as_a_new_revision() {
    let (db, path) = test_db().await;

    let original = save_workflow(&db, &workflow(None, "revertible"))
        .await
        .unwrap();
    let workflow_id = original.id.unwrap();

    let mut broken = original.clone();
    broken.definition = two_node_graph("oops");
    let broken = save_workflow(&db, &broken).await.unwrap();

    let restored = crate::repository::restore_workflow_revision(
        &db,
        workflow_id,
        1,
        &RevisionAuthor::system(RevisionSource::Api),
    )
    .await
    .unwrap();

    // the workflow is back to the original graph...
    assert_eq!(restored.definition, original.definition);
    let current = crate::repository::fetch_workflow(&db, workflow_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.definition, original.definition);

    // ...and history grew rather than being rewritten, so the rollback itself is auditable and
    // the definition it rolled back from is still recoverable.
    let revisions = crate::repository::fetch_workflow_revisions(&db, workflow_id, 50)
        .await
        .unwrap();
    assert_eq!(
        revisions.iter().map(|r| r.revision).collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert_eq!(revisions[0].source, RevisionSource::Rollback);
    assert_eq!(revisions[1].definition, broken.definition);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restoring_preserves_the_current_owner_and_enabled_state() {
    let (db, path) = test_db().await;

    let mut original = workflow(None, "tenanted");
    original.enabled = true;
    let original = save_workflow(&db, &original).await.unwrap();
    let workflow_id = original.id.unwrap();

    let org_id = Uuid::new_v4();
    crate::repository::set_workflow_org(&db, workflow_id, Some(org_id))
        .await
        .unwrap();
    let mut disabled = crate::repository::fetch_workflow(&db, workflow_id)
        .await
        .unwrap()
        .unwrap();
    disabled.enabled = false;
    disabled.definition = two_node_graph("later");
    save_workflow(&db, &disabled).await.unwrap();

    let restored = crate::repository::restore_workflow_revision(
        &db,
        workflow_id,
        1,
        &RevisionAuthor::system(RevisionSource::Api),
    )
    .await
    .unwrap();

    // a rollback restores the *graph*, not the tenancy or the on/off switch: re-enabling a
    // deliberately disabled workflow, or moving it back to a previous org, would be a surprise.
    assert_eq!(restored.definition, original.definition);
    assert_eq!(restored.org_id, Some(org_id));
    assert!(!restored.enabled);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restoring_a_missing_revision_fails_instead_of_saving_something_else() {
    let (db, path) = test_db().await;
    let saved = save_workflow(&db, &workflow(None, "shallow"))
        .await
        .unwrap();

    let result = crate::repository::restore_workflow_revision(
        &db,
        saved.id.unwrap(),
        99,
        &RevisionAuthor::system(RevisionSource::Api),
    )
    .await;
    assert!(result.is_err());

    let _ = std::fs::remove_file(path);
}
