use std::collections::HashMap;

use super::*;
use runinator_models::pipelines::{
    PipelineBundle, PipelineLinkSpec, PipelineRun, PipelineRunDetail, PipelineSpec,
    PipelineTrigger, PipelineTriggerSpec,
};
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};
use runinator_models::workflows::WorkflowTriggerKind;
use uuid::Uuid;

pub async fn upsert_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline: &Pipeline,
) -> Result<Pipeline, SendableError> {
    db.upsert_pipeline(pipeline).await
}

/// import a compiled pipeline bundle from a pack. for each pipeline: resolve member workflow names to
/// ids, upsert the pipeline (reusing an existing id with the same name + org so re-import updates in
/// place), and materialize its links as managed `chained` triggers. pack-managed pipelines carry
/// `metadata.managed_by = "wdl"`, and their link triggers carry `configuration.pipeline_id`.
pub async fn import_pipeline_bundle_with<T: DatabaseImpl>(
    db: &T,
    bundle: &PipelineBundle,
    import_org: Option<Uuid>,
) -> Result<Vec<Pipeline>, SendableError> {
    let existing = db.fetch_pipelines().await?;
    let mut imported = Vec::with_capacity(bundle.pipelines.len());
    for spec in &bundle.pipelines {
        imported.push(import_pipeline_spec(db, spec, import_org, &existing).await?);
    }
    Ok(imported)
}

async fn import_pipeline_spec<T: DatabaseImpl>(
    db: &T,
    spec: &PipelineSpec,
    import_org: Option<Uuid>,
    existing: &[Pipeline],
) -> Result<Pipeline, SendableError> {
    // resolve each member workflow name to its id; an unknown member fails the import loudly.
    let mut workflow_ids = Vec::with_capacity(spec.members.len());
    let mut member_ids: HashMap<&str, Uuid> = HashMap::new();
    for member in &spec.members {
        let id = db
            .fetch_workflow_by_name(member.clone())
            .await?
            .and_then(|workflow| workflow.id)
            .ok_or_else(|| crate::errors::IMPORT_UNKNOWN_PIPELINE_MEMBER.error(member.as_str()))?;
        workflow_ids.push(id);
        member_ids.insert(member.as_str(), id);
    }
    // reuse the id of an existing pipeline with the same name and org so re-import updates in place.
    let prior_id = existing
        .iter()
        .find(|p| p.name == spec.name && p.org_id == import_org)
        .and_then(|p| p.id);
    let pipeline = Pipeline {
        id: prior_id,
        name: spec.name.clone(),
        description: spec.description.clone(),
        org_id: import_org,
        workflow_ids,
        defaults: spec.defaults.clone(),
        metadata: runinator_models::json!({ "managed_by": "wdl" }),
        created_at: None,
        updated_at: None,
    };
    let saved = db.upsert_pipeline(&pipeline).await?;
    let pipeline_id = saved
        .id
        .ok_or_else(|| crate::errors::IMPORT_UNKNOWN_PIPELINE_MEMBER.error(spec.name.as_str()))?;
    materialize_pipeline_links(db, spec, pipeline_id, &member_ids).await?;
    materialize_pipeline_triggers(db, spec, pipeline_id).await?;
    Ok(saved)
}

// realize a pipeline's header triggers as managed `pipeline_triggers`. reconciles idempotently: drop
// this pipeline's prior managed triggers, then insert the current specs. manually-created pipeline
// triggers (no `managed_by == "wdl"`) are left untouched.
async fn materialize_pipeline_triggers<T: DatabaseImpl>(
    db: &T,
    spec: &PipelineSpec,
    pipeline_id: Uuid,
) -> Result<(), SendableError> {
    for existing in db.fetch_pipeline_triggers(pipeline_id).await? {
        let managed = existing
            .metadata
            .pointer("/managed_by")
            .and_then(Value::as_str)
            == Some("wdl");
        if let (true, Some(trigger_id)) = (managed, existing.id) {
            db.delete_pipeline_trigger(trigger_id).await?;
        }
    }
    for spec_trigger in &spec.triggers {
        let trigger = PipelineTrigger {
            id: None,
            pipeline_id,
            kind: spec_trigger.kind.clone(),
            enabled: spec_trigger.enabled,
            configuration: spec_trigger.configuration.clone(),
            next_execution: None,
            blackout_start: None,
            blackout_end: None,
            metadata: runinator_models::json!({ "managed_by": "wdl" }),
            created_at: None,
            updated_at: None,
        };
        db.upsert_pipeline_trigger(&trigger).await?;
    }
    Ok(())
}

// realize a pipeline's links as managed `chained` triggers. reconciles idempotently: on every member
// workflow, drop this pipeline's prior link triggers (keyed by `configuration.pipeline_id`), then
// insert the current links sourced from that workflow. header triggers (no `pipeline_id`) and other
// pipelines' triggers on the same workflow are left untouched.
async fn materialize_pipeline_links<T: DatabaseImpl>(
    db: &T,
    spec: &PipelineSpec,
    pipeline_id: Uuid,
    member_ids: &HashMap<&str, Uuid>,
) -> Result<(), SendableError> {
    let pipeline_key = pipeline_id.to_string();
    let mut by_source: HashMap<Uuid, Vec<&PipelineLinkSpec>> = HashMap::new();
    for link in &spec.links {
        if let Some(from_id) = member_ids.get(link.from.as_str()) {
            by_source.entry(*from_id).or_default().push(link);
        }
    }
    for workflow_id in member_ids.values().copied() {
        for existing in db.fetch_workflow_triggers(workflow_id).await? {
            let belongs = existing
                .configuration
                .pointer("/pipeline_id")
                .and_then(Value::as_str)
                == Some(pipeline_key.as_str());
            if let (true, Some(trigger_id)) = (belongs, existing.id) {
                db.delete_workflow_trigger(trigger_id).await?;
            }
        }
        let Some(links) = by_source.get(&workflow_id) else {
            continue;
        };
        for link in links {
            let trigger = WorkflowTrigger {
                id: None,
                workflow_id,
                kind: WorkflowTriggerKind::Chained,
                enabled: link.enabled,
                configuration: runinator_models::json!({
                    "on": link.on.as_str(),
                    "target_workflow": link.to,
                    "parameters": {},
                    "pipeline_id": pipeline_key,
                }),
                next_execution: None,
                blackout_start: None,
                blackout_end: None,
                metadata: runinator_models::json!({ "managed_by": "wdl" }),
                created_at: None,
                updated_at: None,
            };
            db.upsert_workflow_trigger(&trigger).await?;
        }
    }
    Ok(())
}

pub async fn fetch_pipelines<T: DatabaseImpl>(db: &T) -> Result<Vec<Pipeline>, SendableError> {
    db.fetch_pipelines().await
}

pub async fn fetch_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Option<Pipeline>, SendableError> {
    db.fetch_pipeline(pipeline_id).await
}

pub async fn delete_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline(pipeline_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline deleted".into(),
    })
}

pub async fn set_pipeline_org<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    org_id: Option<Uuid>,
) -> Result<(), SendableError> {
    db.set_pipeline_org(pipeline_id, org_id).await
}

// --- pipeline triggers ---

pub async fn upsert_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger: &PipelineTrigger,
) -> Result<PipelineTrigger, SendableError> {
    db.upsert_pipeline_trigger(trigger).await
}

pub async fn fetch_pipeline_triggers<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Vec<PipelineTrigger>, SendableError> {
    db.fetch_pipeline_triggers(pipeline_id).await
}

pub async fn fetch_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<Option<PipelineTrigger>, SendableError> {
    db.fetch_pipeline_trigger(trigger_id).await
}

pub async fn delete_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline_trigger(trigger_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline trigger deleted".into(),
    })
}

// --- pipeline runs ---

/// start a manual pipeline run for a pipeline id (creates the run and its entry members).
pub async fn create_manual_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    parameters: Value,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let pipeline = db
        .fetch_pipeline(pipeline_id)
        .await?
        .ok_or_else(|| runinator_reducer::errors::PIPELINE_NOT_FOUND.error(pipeline_id))?;
    let provenance = WorkflowRunProvenance {
        source_kind: Some(TriggerSourceKind::Manual),
        actor_type: Some(TriggerActorType::User),
        actor_replica_id,
        actor_display_name,
        request_host: None,
        request_ip: None,
        metadata: Value::Object(Default::default()),
    };
    runinator_reducer::create_and_start_pipeline_run(db, &pipeline, parameters, provenance).await
}

/// start a pipeline run from a manual/cron pipeline trigger id.
pub async fn create_pipeline_run_for_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
    parameters: Value,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let trigger = db
        .fetch_pipeline_trigger(trigger_id)
        .await?
        .ok_or_else(|| runinator_reducer::errors::PIPELINE_TRIGGER_NOT_FOUND.error(trigger_id))?;
    let effective =
        if parameters.is_null() || matches!(&parameters, Value::Object(map) if map.is_empty()) {
            trigger
                .configuration
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()))
        } else {
            parameters
        };
    create_manual_pipeline_run(
        db,
        trigger.pipeline_id,
        effective,
        actor_replica_id,
        actor_display_name,
    )
    .await
}

/// fire due cron pipeline triggers and start each created run's entry members. mirrors the workflow
/// trigger claim wrapper.
pub async fn claim_due_pipeline_trigger_firings<T: DatabaseImpl>(
    db: &T,
    scheduler_id: String,
    limit: i64,
) -> Result<Vec<PipelineRun>, SendableError> {
    let runs = db
        .claim_due_pipeline_trigger_firings(scheduler_id, Utc::now(), limit)
        .await?;
    for run in &runs {
        runinator_reducer::start_pipeline_run(db, run).await?;
    }
    Ok(runs)
}

pub async fn fetch_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<Option<PipelineRun>, SendableError> {
    db.fetch_pipeline_run(pipeline_run_id).await
}

pub async fn fetch_recent_pipeline_runs<T: DatabaseImpl>(
    db: &T,
    limit: i64,
) -> Result<Vec<PipelineRun>, SendableError> {
    db.fetch_recent_pipeline_runs(limit).await
}

/// fetch a pipeline run together with the member workflow runs it started.
pub async fn fetch_pipeline_run_detail<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<Option<PipelineRunDetail>, SendableError> {
    let Some(run) = db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Ok(None);
    };
    let members = db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?;
    Ok(Some(PipelineRunDetail { run, members }))
}

pub async fn fetch_pipeline_runs_for_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Vec<PipelineRun>, SendableError> {
    db.fetch_pipeline_runs_for_pipeline(pipeline_id).await
}

/// cancel a pipeline run and every active member workflow run it owns.
pub async fn cancel_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    for member in db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?
    {
        if member.status.is_active() {
            db.update_workflow_run_status(
                member.id,
                WorkflowStatus::Canceled,
                None,
                None,
                Some("Pipeline run canceled".into()),
            )
            .await?;
        }
    }
    db.update_pipeline_run_status(
        pipeline_run_id,
        WorkflowStatus::Canceled,
        None,
        Some("Pipeline run canceled".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline run canceled".into(),
    })
}
