//! Portable workspace identities, checkout fencing, and atomic effect settlement on every dialect.
use super::*;
use runinator_models::{
    auth::ResourceType,
    rbac::{ResourceOwnership, ScopeRef},
    workspaces::*,
};
use runinator_store::roles::workflow_vm::WorkspaceEffectSettlement;

pub(super) async fn lifecycle<T: DatabaseImpl + WorkflowVmStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) {
    let now = Utc::now();
    let identity = DurableWorkspace {
        id: Uuid::now_v7(),
        key: "parity/workspace".into(),
        org_id: None,
        head_version: 0,
        revision: 1,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    };
    let ownership = ResourceOwnership {
        resource_type: ResourceType::Workspace,
        resource_id: identity.id,
        tenant: ScopeRef::PLATFORM,
        owner: ScopeRef::PLATFORM,
        created_by: None,
        authz_version: 1,
        created_at: now,
        updated_at: now,
    };
    let workspace = db
        .create_durable_workspace(identity.clone(), ownership.clone())
        .await
        .unwrap();
    assert_eq!(workspace, identity);
    let mut duplicate = identity.clone();
    duplicate.id = Uuid::now_v7();
    assert_eq!(
        db.create_durable_workspace(duplicate, ownership)
            .await
            .unwrap()
            .id,
        identity.id
    );
    assert!(
        db.resolve_durable_workspace(Some(Uuid::now_v7()), identity.key.clone())
            .await
            .unwrap()
            .is_none()
    );
    let (run_id, effect_id) = waiting_effect(db, workflow).await;
    let request = WorkspaceAcquire {
        limits: WorkspaceLimits::default(),
        workspace_id: identity.id,
        workflow_run_id: run_id,
        effect_id,
        attempt: 0,
        version: None,
        access: WorkspaceAccess::Write,
        now,
        leased_until: now + Duration::seconds(300),
    };
    let WorkspaceAcquisition::Acquired { checkout } = db
        .acquire_workspace_checkout(request.clone())
        .await
        .unwrap()
    else {
        panic!("checkout");
    };
    assert_eq!(
        db.acquire_workspace_checkout(request.clone())
            .await
            .unwrap(),
        WorkspaceAcquisition::Acquired {
            checkout: checkout.clone()
        }
    );
    let objects: Vec<_> = (0..1001)
        .map(|n| WorkspaceObjectLocation {
            id: format!("{n:064x}"),
            pack: "registered-pack".into(),
            kind: 1,
            raw_len: 1,
            offset: n,
            length: 1,
            member: u32::MAX,
        })
        .collect();
    for batch in objects.chunks(1000) {
        db.stage_workspace_objects(checkout.clone(), batch.to_vec())
            .await
            .unwrap();
    }
    assert_eq!(
        db.fetch_workspace_objects(
            identity.id,
            vec![objects[1000].id.clone(), objects[0].id.clone()]
        )
        .await
        .unwrap()
        .into_iter()
        .map(|object| object.id)
        .collect::<std::collections::HashSet<_>>(),
        [objects[0].id.clone(), objects[1000].id.clone()]
            .into_iter()
            .collect()
    );
    assert!(
        db.fetch_workspace_objects(identity.id, Vec::new())
            .await
            .unwrap()
            .is_empty()
    );
    let first = db
        .workspace_pack_objects(identity.id, "registered-pack".into(), None)
        .await
        .unwrap();
    assert_eq!(first, objects[..1000]);
    let second = db
        .workspace_pack_objects(
            identity.id,
            "registered-pack".into(),
            first.last().map(|o| o.id.clone()),
        )
        .await
        .unwrap();
    assert_eq!(second, objects[1000..]);
    assert!(
        db.workspace_pack_objects(identity.id, "different-pack".into(), None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.workspace_pack_objects(Uuid::now_v7(), "registered-pack".into(), None)
            .await
            .unwrap()
            .is_empty()
    );
    let competing = WorkspaceAcquire {
        effect_id: Uuid::now_v7(),
        ..request.clone()
    };
    assert_eq!(
        db.acquire_workspace_checkout(competing.clone())
            .await
            .unwrap(),
        WorkspaceAcquisition::Busy
    );
    let reader = WorkspaceAcquire {
        effect_id: Uuid::now_v7(),
        access: WorkspaceAccess::Read,
        ..request.clone()
    };
    let WorkspaceAcquisition::Acquired { checkout: reader } =
        db.acquire_workspace_checkout(reader).await.unwrap()
    else {
        panic!("reader");
    };
    assert!(
        db.delete_durable_workspace(identity.id, None)
            .await
            .is_err()
    );
    let snapshot = WorkspaceSnapshot {
        workspace_id: identity.id,
        version: 1,
        parent_version: 0,
        origin: WorkspaceOrigin::Workflow {
            workflow_run_id: run_id,
            effect_id,
            attempt: 0,
        },
        revision_id: "a".repeat(64),
        usage: WorkspaceUsage::default(),
        limits: checkout.limits,
        created_at: now,
    };
    let receipt = WorkspaceReceipt {
        id: Uuid::now_v7(),
        checkout: checkout.clone(),
        snapshot: snapshot.clone(),
    };
    db.save_workspace_receipt(receipt.clone()).await.unwrap();
    let commit = WorkspaceCommit {
        receipt_id: receipt.id,
        checkout: checkout.clone(),
        snapshot,
    };
    let settlement = |commit| WorkspaceEffectSettlement {
        effect_id,
        attempt: 0,
        status: WorkflowEffectStatus::Succeeded,
        output: Some(json!({"workspace": {"key": identity.key, "version": 1}})),
        message: None,
        settled_at: Utc::now(),
        workspace: Some(commit),
    };
    let mut forged = commit.clone();
    forged.receipt_id = Uuid::now_v7();
    assert!(
        db.settle_workflow_effect_with_workspace(settlement(forged))
            .await
            .is_err()
    );
    let mut tampered = commit.clone();
    tampered.snapshot.revision_id = "b".repeat(64);
    assert!(
        db.settle_workflow_effect_with_workspace(settlement(tampered))
            .await
            .is_err()
    );
    let mut stale = commit.clone();
    stale.checkout.fence += 1;
    assert!(
        db.settle_workflow_effect_with_workspace(settlement(stale))
            .await
            .is_err()
    );
    assert_eq!(
        db.fetch_durable_workspace(identity.id)
            .await
            .unwrap()
            .unwrap()
            .head_version,
        0
    );
    assert!(
        db.settle_workflow_effect_with_workspace(settlement(commit.clone()))
            .await
            .unwrap()
    );
    assert!(
        !db.settle_workflow_effect_with_workspace(settlement(commit.clone()))
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_durable_workspace(identity.id)
            .await
            .unwrap()
            .unwrap()
            .head_version,
        1
    );
    assert_eq!(
        db.list_workspace_snapshots(identity.id, 50, 0)
            .await
            .unwrap(),
        vec![commit.snapshot]
    );
    assert!(
        db.delete_durable_workspace(identity.id, Some(1))
            .await
            .is_err()
    );
    let stale_writer = WorkspaceAcquire {
        version: Some(0),
        ..competing.clone()
    };
    assert_eq!(
        db.acquire_workspace_checkout(stale_writer).await.unwrap(),
        WorkspaceAcquisition::Conflict
    );
    db.release_workspace_checkout(reader.id, reader.fence)
        .await
        .unwrap();
    let WorkspaceAcquisition::Acquired { checkout: writer } =
        db.acquire_workspace_checkout(competing).await.unwrap()
    else {
        panic!("new writer");
    };
    assert_eq!(writer.base_version, 1);
    assert!(writer.fence > checkout.fence);
    assert!(
        !db.release_workspace_checkout(writer.id, checkout.fence)
            .await
            .unwrap()
    );
    db.release_workspace_checkout(writer.id, writer.fence)
        .await
        .unwrap();
    assert!(
        db.delete_durable_workspace(identity.id, None)
            .await
            .is_err()
    );
    assert!(
        !db.workspace_gc_candidates()
            .await
            .unwrap()
            .contains(&identity.id)
    );
    assert!(db.claim_workspace_gc(identity.id).await.unwrap().is_none());
    db.update_workflow_run_status(run_id, WorkflowStatus::Succeeded, None, None, None)
        .await
        .unwrap();
    let reader = db.pin_workspace_reader(identity.id, 1).await.unwrap();
    assert!(
        db.delete_durable_workspace(identity.id, None)
            .await
            .is_err()
    );
    assert!(db.renew_workspace_reader(reader.clone()).await.unwrap());
    db.release_workspace_reader(reader.id).await.unwrap();
    assert!(!db.renew_workspace_reader(reader).await.unwrap());
    let export = WorkspaceTransfer {
        filesystem: false,
        id: Uuid::now_v7(),
        workspace_id: identity.id,
        version: 1,
        importing: false,
        state: "queued".into(),
        token: Uuid::nil(),
        bytes_processed: 0,
        archive_uri: None,
        error: None,
        limits: WorkspaceLimits::default(),
        created_at: now,
        expires_at: now + Duration::hours(1),
    };
    db.create_workspace_transfer(export.clone()).await.unwrap();
    assert!(
        db.delete_durable_workspace(identity.id, None)
            .await
            .is_err()
    );
    let running = db
        .claim_workspace_transfer(export.id, false)
        .await
        .unwrap()
        .unwrap();
    assert!(
        db.claim_workspace_transfer(export.id, false)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.progress_workspace_transfer(running.clone(), 100)
            .await
            .unwrap()
    );
    assert_eq!(
        db.fetch_workspace_transfer(export.id)
            .await
            .unwrap()
            .unwrap()
            .bytes_processed,
        100
    );
    assert!(db.cancel_workspace_transfer(export.id).await.unwrap());
    assert!(
        !db.progress_workspace_transfer(running.clone(), 101)
            .await
            .unwrap()
    );
    assert!(
        db.finish_workspace_transfer(
            running,
            "ready".into(),
            Some("blob://workspaces/unused".into()),
            None,
            None
        )
        .await
        .is_err()
    );
    let gc = db.claim_workspace_gc(identity.id).await.unwrap().unwrap();
    assert_eq!(gc.roots, vec!["a".repeat(64)]);
    assert!(db.claim_workspace_gc(identity.id).await.unwrap().is_none());
    let preempting = WorkspaceAcquire {
        effect_id: Uuid::now_v7(),
        now: Utc::now(),
        leased_until: Utc::now() + Duration::minutes(5),
        ..request.clone()
    };
    let WorkspaceAcquisition::Acquired {
        checkout: preempting,
    } = db.acquire_workspace_checkout(preempting).await.unwrap()
    else {
        panic!("foreground writer did not preempt collection");
    };
    db.release_workspace_checkout(preempting.id, preempting.fence)
        .await
        .unwrap();
    assert!(!db.renew_workspace_gc(gc.clone()).await.unwrap());
    assert!(db.finish_workspace_gc(gc, false).await.is_err());
    let gc = db.claim_workspace_gc(identity.id).await.unwrap().unwrap();
    let mut stale_gc = gc.clone();
    stale_gc.fence += 1;
    assert!(db.finish_workspace_gc(stale_gc, false).await.is_err());
    assert!(db.renew_workspace_gc(gc.clone()).await.unwrap());
    db.finish_workspace_gc(gc.clone(), false).await.unwrap();
    assert!(db.finish_workspace_gc(gc, false).await.is_err());
    assert!(
        db.delete_durable_workspace(identity.id, None)
            .await
            .unwrap()
    );
    assert!(
        db.fetch_durable_workspace(identity.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_workspace_snapshot(identity.id, 1)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(db.pending_workspace_cleanup().await.unwrap().len(), 1);
    db.finish_workspace_cleanup(identity.id, 1).await.unwrap();
    assert!(db.pending_workspace_cleanup().await.unwrap().is_empty());
    let mut imported = identity.clone();
    imported.id = Uuid::now_v7();
    imported.key = "parity/imported-workspace".into();
    let ownership = ResourceOwnership {
        resource_type: ResourceType::Workspace,
        resource_id: imported.id,
        tenant: ScopeRef::PLATFORM,
        owner: ScopeRef::PLATFORM,
        created_by: None,
        authz_version: 1,
        created_at: now,
        updated_at: now,
    };
    db.create_durable_workspace(imported.clone(), ownership)
        .await
        .unwrap();
    let importing = WorkspaceTransfer {
        filesystem: false,
        id: Uuid::now_v7(),
        workspace_id: imported.id,
        version: 0,
        importing: true,
        state: "uploading".into(),
        token: Uuid::nil(),
        bytes_processed: 0,
        archive_uri: None,
        error: None,
        limits: WorkspaceLimits::default(),
        created_at: now,
        expires_at: now + Duration::hours(1),
    };
    db.create_workspace_transfer(importing.clone())
        .await
        .unwrap();
    assert!(db.claim_workspace_gc(imported.id).await.unwrap().is_none());
    let upload = db
        .claim_workspace_transfer(importing.id, true)
        .await
        .unwrap()
        .unwrap();
    db.finish_workspace_transfer(
        upload,
        "queued".into(),
        Some("blob://runinator-workspaces/source".into()),
        None,
        None,
    )
    .await
    .unwrap();
    let working = db
        .claim_workspace_transfer(importing.id, false)
        .await
        .unwrap()
        .unwrap();
    let snapshot = WorkspaceSnapshot {
        workspace_id: imported.id,
        version: 1,
        parent_version: 0,
        origin: WorkspaceOrigin::Import {
            transfer_id: importing.id,
            format: "oci".into(),
        },
        revision_id: "b".repeat(64),
        usage: WorkspaceUsage::default(),
        limits: importing.limits,
        created_at: now,
    };
    db.finish_workspace_transfer(
        working.clone(),
        "ready".into(),
        None,
        Some(snapshot.clone()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        db.fetch_durable_workspace(imported.id)
            .await
            .unwrap()
            .unwrap()
            .head_version,
        1
    );
    assert_eq!(
        db.fetch_workspace_snapshot(imported.id, 1).await.unwrap(),
        Some(snapshot.clone())
    );
    assert!(
        db.finish_workspace_transfer(working, "ready".into(), None, Some(snapshot), None)
            .await
            .is_err()
    );
}

async fn waiting_effect<T: DatabaseImpl + WorkflowVmStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> (Uuid, Uuid) {
    let snapshot = db
        .fetch_workflow(workflow.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    let run = db
        .create_workflow_run(
            snapshot.id.unwrap(),
            snapshot,
            Value::Null,
            Value::Null,
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Action {
                provider: "test".into(),
                function: "workspace".into(),
                input: Value::Null,
                timeout_seconds: Some(300),
                retry: Default::default(),
                tags: vec![],
                required_labels: Default::default(),
                workspace_affinity: Some(json!({"key": "parity/workspace"})),
                execution_profile: None,
                idempotency_key: None,
                function_binding: None,
            },
        },
        WorkflowInstruction::Return,
    ]);
    let root = WorkflowContinuation::start(run.id, module.version);
    db.create_workflow_vm(module.clone(), root.clone())
        .await
        .unwrap();
    let claimed = db
        .claim_runnable_workflow_continuations(
            "workspace-parity".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            1000,
        )
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == root.id)
        .unwrap();
    let runinator_runtime::WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        request,
    } = runinator_runtime::step_workflow_vm(&module, claimed)
    else {
        panic!("effect yield");
    };
    let now = Utc::now().timestamp();
    let effect = WorkflowEffect {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        id: effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        sequence,
        attempt: 0,
        node_id: None,
        timeline_category: Default::default(),
        request: *request.clone(),
        status: WorkflowEffectStatus::Requested,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
        result: None,
        message: None,
        created_at: now,
        updated_at: now,
        finished_at: None,
    };
    let command = EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        attempt: 0,
        request: *request,
        executor: runinator_comm::EffectExecutor::Provider,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: effect.idempotency_key(),
        notification_delivery_id: None,
    };
    db.suspend_on_effect(continuation, effect, command)
        .await
        .unwrap();
    (run.id, effect_id)
}
