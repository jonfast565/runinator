use super::*;
use super::{catalog, triggers};
use runinator_models::semver::SemVerBump;
use uuid::Uuid;

/// shallow-merge `parameters` over `defaults` when both are json objects; used by task-parameter
/// defaulting tests, including cross-crate ones under the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub fn merge_json_object(defaults: &Value, parameters: &Value) -> Value {
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

pub async fn upsert_workflow<T: DefinitionStore + RuntimeStore + FunctionStore>(
    db: &T,
    workflow: &WorkflowDefinition,
    author: &RevisionAuthor,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = validate_workflow_definition_with_catalog(db, workflow).await?;
    let saved = db.upsert_workflow(&workflow).await?;
    record_workflow_revision(db, &saved, author).await;
    Ok(saved)
}

/// capture an accepted definition as an immutable revision.
///
/// best-effort, like the audit trail: history is worth having, but failing a legitimate save
/// because it could not be written would be the worse outcome. a `None` result is the store
/// reporting the definition was unchanged, not a failure.
pub async fn record_workflow_revision<T: DefinitionStore>(
    db: &T,
    saved: &WorkflowDefinition,
    author: &RevisionAuthor,
) {
    let Some(workflow_id) = saved.id else {
        return;
    };
    let revision = WorkflowRevision {
        id: Uuid::nil(),
        workflow_id,
        revision: 0,
        version: saved.version,
        name: saved.name.clone(),
        input_type: saved.input_type.clone(),
        definition: saved.definition.clone(),
        source: author.source,
        actor_id: author.actor_id,
        actor_kind: author.actor_kind.clone(),
        note: author.note.clone(),
        created_at: None,
    };
    if let Err(err) = db.insert_workflow_revision(&revision).await {
        log::warn!(
            "failed to record revision for workflow {workflow_id} ('{}'): {err}",
            saved.name
        );
    }
}

pub async fn fetch_workflow_revisions<T: DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
    limit: i64,
) -> Result<Vec<WorkflowRevision>, SendableError> {
    db.fetch_workflow_revisions(workflow_id, limit).await
}

pub async fn fetch_workflow_revision<T: DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
    revision: i64,
) -> Result<Option<WorkflowRevision>, SendableError> {
    db.fetch_workflow_revision(workflow_id, revision).await
}

/// restore an earlier revision as the workflow's current definition.
///
/// the restore is a forward write, never a rewrite of history: the old definition is re-validated
/// against today's provider catalog and saved as a *new* revision. that is what stops a rollback
/// from resurrecting a definition referencing an action that has since been removed — it fails
/// loudly instead of persisting something that cannot run.
pub async fn restore_workflow_revision<T: DefinitionStore + RuntimeStore + FunctionStore>(
    db: &T,
    workflow_id: Uuid,
    revision: i64,
    author: &RevisionAuthor,
) -> Result<WorkflowDefinition, SendableError> {
    let Some(current) = fetch_workflow(db, workflow_id).await? else {
        return Err(
            runinator_runtime::errors::WORKFLOW_NOT_FOUND.error(format!("id {workflow_id}"))
        );
    };
    let Some(target) = db.fetch_workflow_revision(workflow_id, revision).await? else {
        return Err(runinator_runtime::errors::WORKFLOW_NOT_FOUND
            .error(format!("workflow {workflow_id} has no revision {revision}")));
    };

    let restored = target.to_definition(&current);
    let author = RevisionAuthor {
        source: RevisionSource::Rollback,
        note: author
            .note
            .clone()
            .or_else(|| Some(format!("restored revision {revision}"))),
        ..author.clone()
    };
    upsert_workflow(db, &restored, &author).await
}

pub async fn validate_workflow_definition_with_catalog<
    T: DefinitionStore + RuntimeStore + FunctionStore,
>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = validate_workflow_definition(workflow)?;
    let providers = catalog::fetch_catalog_items(db, Some("provider_metadata".into())).await?;
    let providers = provider_metadata_from_items(providers)?;
    // type-check `config.*` references against the stored settings schema.
    let config_type = crate::settings::config_type_tree(db).await;
    runinator_workflows::validate_workflow_with_config(&workflow, &providers, &config_type)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    validate_workflow_subflows(db, &workflow).await?;
    validate_workflow_function_bindings(db, &workflow).await?;
    Ok(workflow)
}

/// every packaged-function binding in a definition must name a version that still exists, in an
/// org the workflow may reach.
///
/// placed here rather than in a handler so the UI save, `import_rexrap`, a pack import, and a revision
/// rollback are all covered by the same check — a rollback in particular can restore a definition
/// whose package was deleted since, and a run that discovers that at dispatch time is far worse
/// than a save that refuses it.
async fn validate_workflow_function_bindings<T: FunctionStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    let bindings: Vec<_> = workflow
        .definition
        .nodes
        .iter()
        .flat_map(|node| {
            node.action
                .iter()
                .chain(node.compensation.iter())
                .filter_map(|action| {
                    action
                        .function_binding
                        .as_ref()
                        .map(|binding| (node.id.clone(), binding))
                })
        })
        .collect();
    if bindings.is_empty() {
        return Ok(());
    }
    for (node_id, binding) in bindings {
        let Some(export) = db.fetch_function_export(binding.export_id).await? else {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', which is no longer published",
                binding.call_path()
            )));
        };
        let Some(version) = db.fetch_function_version(export.version_id).await? else {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', whose version is missing",
                binding.call_path()
            )));
        };
        // the binding pins a digest, so a version that somehow points at different bytes is a
        // different thing than the workflow was compiled against.
        if version.artifact_digest != binding.artifact_digest {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' pins artifact {} for '{}', but that version now holds {}",
                binding.artifact_digest,
                binding.call_path(),
                version.artifact_digest
            )));
        }
        let Some(package) = db.fetch_function_package_by_id(version.package_id).await? else {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', whose package is missing",
                binding.call_path()
            )));
        };
        // a global package (`None`) is reachable from any org; an org-owned one only from its own.
        // note the direction: a `None` *workflow* org must not match every package, or a
        // platform-scoped workflow would be able to call another tenant's private code.
        let reachable = package.org_id.is_none() || package.org_id == workflow.org_id;
        if !reachable {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', which belongs to another organization",
                binding.call_path()
            )));
        }
    }
    Ok(())
}

async fn validate_workflow_subflows<T: RuntimeStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    for node in &workflow.definition.nodes {
        if node.kind == WorkflowNodeKind::Subflow
            && let Some(subflow_id) = node.subflow_id
            && !subflow_id.is_nil()
        {
            match db.fetch_workflow(subflow_id).await {
                Ok(Some(_)) => {} // workflow exists, validation passes
                _ => {
                    return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                        "node '{}' references non-existent workflow with id {subflow_id}",
                        node.id
                    )));
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

/// every authored workflow.
///
/// generated workflows (a packaged function's adapter) are filtered out: they exist so an http
/// invocation has a run to start, and listing them would put one entry per published export into a
/// workflow list nobody authored.
pub async fn fetch_workflows<T: DefinitionStore>(
    db: &T,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    Ok(fetch_workflows_with_managed(db, false).await?)
}

/// every workflow, optionally including the generated ones.
///
/// filtered in rust after the fetch, the way the org and grant filters already are, rather than as
/// a json predicate in sql — the marker lives inside a json column and the three dialects do not
/// agree on how to reach into one.
pub async fn fetch_workflows_with_managed<T: DefinitionStore>(
    db: &T,
    include_managed: bool,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    let workflows = db.fetch_workflows().await?;
    let mut normalized = Vec::with_capacity(workflows.len());
    for workflow in workflows {
        let workflow = normalize_persisted_workflow(db, workflow).await?;
        // both generated kinds are hidden by the same rule: a function adapter and a console
        // scratch workflow are each one row per published export / per cell run, and either alone
        // would bury the authored workflows.
        let generated = super::function_adapters::is_adapter_workflow(&workflow)
            || super::console::is_console_workflow(&workflow);
        if include_managed || !generated {
            normalized.push(workflow);
        }
    }
    Ok(normalized)
}

pub async fn fetch_workflow<T: RuntimeStore + DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    let Some(workflow) = db.fetch_workflow(workflow_id).await? else {
        return Ok(None);
    };
    Ok(Some(normalize_persisted_workflow(db, workflow).await?))
}

pub async fn set_workflow_org<T: DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
    org_id: Option<Uuid>,
) -> Result<(), SendableError> {
    db.set_workflow_org(workflow_id, org_id).await
}

pub async fn fetch_workflow_by_name<T: RuntimeStore + DefinitionStore>(
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

pub async fn import_workflow_bundle<
    T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore,
>(
    db: &T,
    bundle: WorkflowBundle,
) -> Result<WorkflowBundle, SendableError> {
    import_workflow_bundle_with(db, bundle, false).await
}

// `overwrite` makes an explicit re-apply authoritative: existing items are updated in place even
// when the incoming copy is not strictly newer, bypassing the reconciliation timestamp gate that
// background sync relies on. callers that reconcile (gossip, plain imports) pass `false`.
pub async fn import_workflow_bundle_with<
    T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore,
>(
    db: &T,
    bundle: WorkflowBundle,
    overwrite: bool,
) -> Result<WorkflowBundle, SendableError> {
    // reject the whole pack up front if any subflow targets a workflow that is neither in the pack
    // nor already stored, so a typo'd `spawn "Naem"` fails at apply time rather than at run time.
    validate_subflow_targets(db, &bundle).await?;
    // likewise reject a chaining trigger whose target workflow cannot be resolved.
    validate_chained_targets(db, &bundle).await?;

    // a pack apply overwrites definitions wholesale, which is exactly the change most worth being
    // able to see and undo later. the store drops a revision whose definition is unchanged, so a
    // pack that reapplies on a schedule leaves history alone.
    let pack_author = RevisionAuthor::system(RevisionSource::Pack);
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
        let imported = upsert_workflow(db, &workflow, &pack_author).await?;
        // materialize this workflow's declared `trigger cron` schedules (idempotent).
        materialize_workflow_triggers(db, &imported).await?;
        // and its declared `notify on ...` alerting policies, reconciled the same way.
        materialize_workflow_notifications(db, &imported).await?;
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
async fn validate_subflow_targets<T: RuntimeStore>(
    db: &T,
    bundle: &WorkflowBundle,
) -> Result<(), SendableError> {
    // a subflow target may be unqualified (`"name"`) or qualified (`"<namespace>.<name>"`); accept
    // both forms for every workflow in the bundle so cross-workflow references resolve at import.
    let mut incoming: std::collections::HashSet<String> = std::collections::HashSet::new();
    for workflow in &bundle.workflows {
        incoming.insert(workflow.name.clone());
        if let Some(namespace) = &workflow.namespace {
            incoming.insert(format!("{namespace}.{}", workflow.name));
        }
    }
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
            return Err(crate::errors::IMPORT_UNKNOWN_SUBFLOW.error(format!(
                "workflow '{}' references unknown subflow workflow '{name}'",
                workflow.name
            )));
        }
    }
    Ok(())
}

/// validate that every chaining trigger targets a workflow present in the bundle or already stored.
async fn validate_chained_targets<T: RuntimeStore>(
    db: &T,
    bundle: &WorkflowBundle,
) -> Result<(), SendableError> {
    let mut incoming: std::collections::HashSet<String> = std::collections::HashSet::new();
    for workflow in &bundle.workflows {
        incoming.insert(workflow.name.clone());
        if let Some(namespace) = &workflow.namespace {
            incoming.insert(format!("{namespace}.{}", workflow.name));
        }
    }
    for workflow in &bundle.workflows {
        let specs = workflow
            .definition
            .metadata
            .pointer("/triggers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for spec in &specs {
            if spec.get("kind").and_then(Value::as_str) != Some("chained") {
                continue;
            }
            let Some(name) = spec
                .get("target_workflow")
                .and_then(Value::as_str)
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
            return Err(crate::errors::IMPORT_UNKNOWN_CHAINED_TARGET.error(format!(
                "workflow '{}' chains to unknown target workflow '{name}'",
                workflow.name
            )));
        }
    }
    Ok(())
}

/// replace a workflow's `managed_by: rexrap` notification policies with the ones declared in its
/// `definition.metadata.notifications`. hand-authored policies on the same workflow are left
/// untouched, and re-import is idempotent, mirroring how managed triggers reconcile.
async fn materialize_workflow_notifications<T: NotificationStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    let Some(workflow_id) = workflow.id else {
        return Ok(());
    };
    let specs = workflow
        .definition
        .metadata
        .pointer("/notifications")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut policies = Vec::with_capacity(specs.len());
    for spec in &specs {
        let event = spec
            .get("event")
            .and_then(Value::as_str)
            .and_then(|event| NotificationEvent::try_from(event).ok())
            .unwrap_or(NotificationEvent::RunFailed);
        let channel = spec
            .get("channel")
            .and_then(Value::as_str)
            .and_then(|channel| NotificationChannel::try_from(channel).ok())
            .unwrap_or(NotificationChannel::InApp);
        let severity = spec
            .get("severity")
            .and_then(Value::as_str)
            .and_then(|severity| NotificationSeverity::try_from(severity).ok())
            .unwrap_or(NotificationSeverity::Warning);
        policies.push(NewNotificationPolicy {
            workflow_id: Some(workflow_id),
            // the declaration has no name of its own; derive a stable, readable one.
            name: format!("{} {}", workflow.name, event.as_str()),
            event,
            severity,
            channel,
            target: spec
                .get("target")
                .and_then(Value::as_str)
                .map(str::to_string),
            threshold_seconds: spec.get("threshold_seconds").and_then(Value::as_i64),
            enabled: spec.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            managed_by: Some("rexrap".into()),
            configuration: spec.get("configuration").cloned().unwrap_or(Value::Null),
        });
    }
    // always call through, even with an empty list: removing the last `notify` line from a pack must
    // delete the policy it previously materialized.
    db.replace_managed_notification_policies(workflow_id, "rexrap".into(), policies)
        .await
}

/// replace a workflow's `managed_by: rexrap` triggers with the ones declared in its
/// `definition.metadata.triggers`. manually-added triggers are left untouched; re-import is
/// idempotent (delete the pack-managed set, then insert the current declarations). each spec's
/// `kind` selects the trigger kind (absent ⇒ `cron` for back-compat with older packs).
async fn materialize_workflow_triggers<T: ScheduleStore + RuntimeStore>(
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
    // drop the previous pack-managed header triggers for this workflow. pipeline-link triggers are
    // also managed_by=rexrap but owned by a pipeline (keyed by configuration.pipeline_id); leave those
    // to pipeline reconciliation so materializing a workflow does not clobber its pipeline links.
    for existing in db.fetch_workflow_triggers(workflow_id).await? {
        let managed = existing
            .metadata
            .pointer("/managed_by")
            .and_then(Value::as_str)
            == Some("rexrap");
        let pipeline_owned = existing.configuration.pointer("/pipeline_id").is_some();
        if let (true, false, Some(trigger_id)) = (managed, pipeline_owned, existing.id) {
            db.delete_workflow_trigger(trigger_id).await?;
        }
    }
    // insert the currently declared triggers.
    for spec in &specs {
        let parameters = spec
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let kind = spec.get("kind").and_then(Value::as_str).unwrap_or("cron");
        let trigger = match kind {
            "chained" => {
                let Some(target) = spec
                    .get("target_workflow")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                else {
                    continue;
                };
                let on = spec.get("on").and_then(Value::as_str).unwrap_or("success");
                WorkflowTrigger {
                    id: None,
                    workflow_id,
                    kind: runinator_models::workflows::WorkflowTriggerKind::Chained,
                    enabled,
                    configuration: runinator_models::json!({
                        "on": on,
                        "target_workflow": target,
                        "parameters": parameters,
                    }),
                    next_execution: None,
                    blackout_start: None,
                    blackout_end: None,
                    metadata: runinator_models::json!({ "managed_by": "rexrap" }),
                    created_at: None,
                    updated_at: None,
                }
            }
            // absent kind ⇒ cron for back-compat with packs compiled before the kind discriminator.
            _ => {
                let Some(cron) = spec
                    .get("cron")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                else {
                    continue;
                };
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
                let mut configuration =
                    runinator_models::json!({ "cron": cron, "parameters": parameters });
                // the catch-up policy travels in the trigger's configuration, so a pack that drops
                // the `catchup` clause reverts the trigger to the default on the next import.
                if let (Some(catchup), Some(object)) =
                    (spec.get("catchup"), configuration.as_object_mut())
                {
                    object.insert("catchup".into(), catchup.clone());
                }
                WorkflowTrigger {
                    id: None,
                    workflow_id,
                    kind: runinator_models::workflows::WorkflowTriggerKind::Cron,
                    enabled,
                    configuration,
                    next_execution: None,
                    blackout_start,
                    blackout_end,
                    metadata: runinator_models::json!({ "managed_by": "rexrap" }),
                    created_at: None,
                    updated_at: None,
                }
            }
        };
        db.upsert_workflow_trigger(&trigger).await?;
    }
    Ok(())
}

fn parse_trigger_datetime(value: &str) -> Result<DateTime<Utc>, SendableError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| {
            crate::errors::IMPORT_INVALID_TRIGGER_BLACKOUT.error(format!("'{value}': {err}"))
        })
}

pub async fn export_workflow_bundle<T: DefinitionStore + RuntimeStore + ScheduleStore>(
    db: &T,
    workflow_id: Option<Uuid>,
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

async fn normalize_persisted_workflow<T: DefinitionStore>(
    db: &T,
    workflow: WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let normalized = runinator_workflows::normalize_workflow(&workflow);
    if normalized.definition == workflow.definition {
        return Ok(workflow);
    }
    // deliberately writes past the revision-recording path: this fires on a *read* and only rewrites
    // a stored definition into its canonical form. it is a lazy migration, not an authored change,
    // and recording it would fill a workflow's history with revisions nobody made.
    db.upsert_workflow(&normalized).await
}

// duplicate a workflow into a new row that shares its name but carries a bumped semantic
// version. the copy is a fresh, disabled draft (new id) so it never clobbers the original or
// inherits its triggers; the highest-versioned sibling is left to the caller to promote.
pub async fn duplicate_workflow<T: DefinitionStore + RuntimeStore + FunctionStore>(
    db: &T,
    workflow_id: Uuid,
    bump: SemVerBump,
    author: &RevisionAuthor,
) -> Result<WorkflowDefinition, SendableError> {
    let Some(existing) = fetch_workflow(db, workflow_id).await? else {
        return Err(
            runinator_runtime::errors::WORKFLOW_NOT_FOUND.error(format!("id {workflow_id}"))
        );
    };
    let mut copy = existing;
    copy.id = None;
    copy.version = copy.version.bump(bump);
    copy.enabled = false;
    copy.created_at = None;
    copy.updated_at = None;
    let copy = validate_workflow_definition_with_catalog(db, &copy).await?;
    let created = db.insert_workflow(&copy).await?;
    // the copy is a new workflow, so it starts its own history at revision 1 rather than inheriting
    // the original's — the two rows diverge from here.
    let author = RevisionAuthor {
        source: RevisionSource::Duplicate,
        note: author
            .note
            .clone()
            .or_else(|| Some(format!("duplicated from workflow {workflow_id}"))),
        ..author.clone()
    };
    record_workflow_revision(db, &created, &author).await;
    Ok(created)
}

pub async fn delete_workflow<T: DefinitionStore>(
    db: &T,
    workflow_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow(workflow_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow deleted".into(),
    })
}
