use super::*;
use super::{catalog, triggers};

#[cfg(test)]
pub(crate) fn merge_json_object(defaults: &Value, parameters: &Value) -> Value {
    match (defaults, parameters) {
        (Value::Object(defaults), Value::Object(parameters)) => {
            let mut merged = defaults.clone();
            for (key, value) in parameters {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        (_, Value::Null) => defaults.clone(),
        _ => parameters.clone(),
    }
}

pub async fn upsert_workflow<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = validate_workflow_definition_with_catalog(db, workflow).await?;
    db.upsert_workflow(&workflow).await
}

pub async fn validate_workflow_definition_with_catalog<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = validate_workflow_definition(workflow)?;
    let providers = catalog::fetch_catalog_items(db, Some("provider_metadata".into())).await?;
    let providers = provider_metadata_from_items(providers)?;
    runinator_workflows::validate_workflow_with_providers(&workflow, &providers)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    validate_workflow_subflows(db, &workflow).await?;
    Ok(workflow)
}

async fn validate_workflow_subflows<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    for node in &workflow.definition.nodes {
        if node.kind == WorkflowNodeKind::Subflow
            && let Some(subflow_id) = node.subflow_id
            && subflow_id > 0
        {
            match db.fetch_workflow(subflow_id).await {
                Ok(Some(_)) => {} // workflow exists, validation passes
                _ => {
                    let err = RuntimeError::new(
                        "workflow.subflow.invalid_id".into(),
                        format!(
                            "Node '{}' references non-existent workflow with id {subflow_id}",
                            node.id
                        ),
                    );
                    return Err(Box::new(err));
                }
            }
        }
    }
    Ok(())
}

pub fn validate_workflow_definition(
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = runinator_workflows::normalize_workflow(workflow);
    runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    Ok(workflow)
}

pub async fn fetch_workflows<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    let workflows = db.fetch_workflows().await?;
    let mut normalized = Vec::with_capacity(workflows.len());
    for workflow in workflows {
        normalized.push(normalize_persisted_workflow(db, workflow).await?);
    }
    Ok(normalized)
}

pub async fn fetch_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    let Some(workflow) = db.fetch_workflow(workflow_id).await? else {
        return Ok(None);
    };
    Ok(Some(normalize_persisted_workflow(db, workflow).await?))
}

pub async fn fetch_workflow_by_name<T: DatabaseImpl>(
    db: &T,
    name: String,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    let Some(workflow) = db.fetch_workflow_by_name(name).await? else {
        return Ok(None);
    };
    Ok(Some(normalize_persisted_workflow(db, workflow).await?))
}

// true when an incoming record should overwrite the stored one: it must carry a
// timestamp that is strictly newer than the stored copy. a missing incoming timestamp
// never overwrites; a missing stored timestamp is treated as oldest.
fn incoming_is_newer(incoming: Option<DateTime<Utc>>, stored: Option<DateTime<Utc>>) -> bool {
    match (incoming, stored) {
        (Some(incoming), Some(stored)) => incoming > stored,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

pub async fn import_workflow_bundle<T: DatabaseImpl>(
    db: &T,
    bundle: WorkflowBundle,
) -> Result<WorkflowBundle, SendableError> {
    import_workflow_bundle_with(db, bundle, false).await
}

// `overwrite` makes an explicit re-apply authoritative: existing items are updated in place even
// when the incoming copy is not strictly newer, bypassing the reconciliation timestamp gate that
// background sync relies on. callers that reconcile (gossip, plain imports) pass `false`.
pub async fn import_workflow_bundle_with<T: DatabaseImpl>(
    db: &T,
    bundle: WorkflowBundle,
    overwrite: bool,
) -> Result<WorkflowBundle, SendableError> {
    // reject the whole pack up front if any subflow targets a workflow that is neither in the pack
    // nor already stored, so a typo'd `spawn "Naem"` fails at apply time rather than at run time.
    validate_subflow_targets(db, &bundle).await?;

    let mut workflows = Vec::with_capacity(bundle.workflows.len());
    for workflow in bundle.workflows {
        // an incoming id is an explicit save (e.g. the command center) and always wins.
        // an id-less workflow is a pack import: unless this is an explicit overwrite, update an
        // existing workflow only when the incoming copy carries a strictly newer updated_at, so a
        // background reconcile does not clobber a workflow the user has since modified.
        if !overwrite
            && workflow.id.is_none()
            && let Some(existing) = db.fetch_workflow_by_name(workflow.name.clone()).await?
            && !incoming_is_newer(workflow.updated_at, existing.updated_at)
        {
            log::info!(
                "Skipping import of workflow '{}': stored copy is up to date",
                workflow.name
            );
            workflows.push(existing);
            continue;
        }
        let imported = upsert_workflow(db, &workflow).await?;
        // materialize this workflow's declared `trigger cron` schedules (idempotent).
        materialize_workflow_triggers(db, &imported).await?;
        workflows.push(imported);
    }

    let mut triggers = Vec::with_capacity(bundle.triggers.len());
    for trigger in bundle.triggers {
        triggers.push(triggers::upsert_workflow_trigger(db, &trigger).await?);
    }

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}

/// validate that every subflow node targets a workflow present in the bundle or already stored.
async fn validate_subflow_targets<T: DatabaseImpl>(
    db: &T,
    bundle: &WorkflowBundle,
) -> Result<(), SendableError> {
    let incoming: std::collections::HashSet<&str> =
        bundle.workflows.iter().map(|w| w.name.as_str()).collect();
    for workflow in &bundle.workflows {
        // structural problems surface in the per-workflow validator/upsert; skip them here.
        let Ok((_, nodes)) = runinator_workflows::parse_nodes(workflow) else {
            continue;
        };
        for node in nodes {
            if node.kind != WorkflowNodeKind::Subflow || node.subflow_id.is_some() {
                continue;
            }
            let Some(name) = node
                .subflow
                .workflow_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            if incoming.contains(name)
                || db.fetch_workflow_by_name(name.to_string()).await?.is_some()
            {
                continue;
            }
            return Err(Box::new(RuntimeError::new(
                "workflow.import.unknown_subflow".into(),
                format!(
                    "workflow '{}' references unknown subflow workflow '{name}'",
                    workflow.name
                ),
            )));
        }
    }
    Ok(())
}

/// replace a workflow's `managed_by: wdl` cron triggers with the ones declared in its
/// `definition.metadata.triggers`. manually-added triggers are left untouched; re-import is
/// idempotent (delete the pack-managed set, then insert the current declarations).
async fn materialize_workflow_triggers<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    let Some(workflow_id) = workflow.id else {
        return Ok(());
    };
    let specs = workflow
        .definition
        .metadata
        .pointer("/triggers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // drop the previous pack-managed triggers for this workflow.
    for existing in db.fetch_workflow_triggers(workflow_id).await? {
        let managed = existing
            .metadata
            .pointer("/managed_by")
            .and_then(Value::as_str)
            == Some("wdl");
        if let (true, Some(trigger_id)) = (managed, existing.id) {
            db.delete_workflow_trigger(trigger_id).await?;
        }
    }
    // insert the currently declared schedules.
    for spec in &specs {
        let Some(cron) = spec
            .get("cron")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let parameters = spec
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let blackout_start = spec
            .get("blackout_start")
            .and_then(Value::as_str)
            .map(parse_trigger_datetime)
            .transpose()?;
        let blackout_end = spec
            .get("blackout_end")
            .and_then(Value::as_str)
            .map(parse_trigger_datetime)
            .transpose()?;
        let trigger = WorkflowTrigger {
            id: None,
            workflow_id,
            kind: runinator_models::workflows::WorkflowTriggerKind::Cron,
            enabled,
            configuration: runinator_models::json!({ "cron": cron, "parameters": parameters }),
            next_execution: None,
            blackout_start,
            blackout_end,
            metadata: runinator_models::json!({ "managed_by": "wdl" }),
            created_at: None,
            updated_at: None,
        };
        db.upsert_workflow_trigger(&trigger).await?;
    }
    Ok(())
}

fn parse_trigger_datetime(value: &str) -> Result<DateTime<Utc>, SendableError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| {
            Box::new(RuntimeError::new(
                "workflow.import.invalid_trigger_blackout".into(),
                format!("invalid trigger blackout datetime '{value}': {err}"),
            )) as SendableError
        })
}

pub async fn export_workflow_bundle<T: DatabaseImpl>(
    db: &T,
    workflow_id: Option<i64>,
) -> Result<WorkflowBundle, SendableError> {
    let workflows = match workflow_id {
        Some(id) => match fetch_workflow(db, id).await? {
            Some(workflow) => vec![workflow],
            None => return Ok(WorkflowBundle::default()),
        },
        None => fetch_workflows(db).await?,
    };

    let mut triggers = Vec::new();
    for workflow in &workflows {
        let Some(id) = workflow.id else {
            continue;
        };
        triggers.extend(triggers::fetch_workflow_triggers(db, id).await?);
    }

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}

async fn normalize_persisted_workflow<T: DatabaseImpl>(
    db: &T,
    workflow: WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let normalized = runinator_workflows::normalize_workflow(&workflow);
    if normalized.definition == workflow.definition {
        return Ok(workflow);
    }
    db.upsert_workflow(&normalized).await
}

pub async fn delete_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow(workflow_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow deleted".into(),
    })
}
