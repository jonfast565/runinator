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
    let mut workflows = Vec::with_capacity(bundle.workflows.len());
    for workflow in bundle.workflows {
        // an incoming id is an explicit save (e.g. the command center) and always wins.
        // an id-less workflow is a pack import: overwrite an existing workflow only when
        // the incoming copy carries a strictly newer updated_at, so we do not clobber a
        // workflow the user has since modified.
        if workflow.id.is_none()
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
        workflows.push(upsert_workflow(db, &workflow).await?);
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
