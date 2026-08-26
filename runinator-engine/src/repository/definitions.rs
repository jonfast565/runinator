use super::*;
use super::{catalog, triggers};
use runinator_models::artifacts::{ArtifactKind, ArtifactPath, ArtifactRef};
use runinator_models::semver::SemVerBump;
use runinator_models::settings::{SettingBinding, SettingKind};
use runinator_models::workflows::WorkflowGraph;
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

fn invalid_definition(message: impl Into<String>) -> SendableError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

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
    let workflow = prepare_workflows_for_save(db, vec![workflow.clone()])
        .await?
        .pop()
        .expect("one workflow was supplied");
    let mut bundle = WorkflowBundle {
        workflows: vec![workflow],
        triggers: Vec::new(),
    };
    resolve_chained_targets(db, &mut bundle).await?;
    let workflow = bundle.workflows.pop().expect("one workflow was supplied");
    let known_subflows = workflow.id.into_iter().collect();
    upsert_prepared_workflow(db, &workflow, &known_subflows, author).await
}

/// Save a definition whose artifact references were already resolved as one pack-wide operation.
/// `known_subflows` permits mutually-recursive workflows in the same pack: their UUIDs are
/// preassigned and committed exactly once, instead of using a name lookup or creating a second
/// revision just to backfill their edges.
async fn upsert_prepared_workflow<T: DefinitionStore + RuntimeStore + FunctionStore>(
    db: &T,
    workflow: &WorkflowDefinition,
    known_subflows: &HashSet<Uuid>,
    author: &RevisionAuthor,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow =
        validate_workflow_definition_with_catalog_and_known_subflows(db, workflow, known_subflows)
            .await?;
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
        digest: WorkflowRevision::content_digest(
            saved.version,
            &saved.input_type,
            &saved.definition,
        ),
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
    validate_workflow_definition_with_catalog_and_known_subflows(db, workflow, &HashSet::new())
        .await
}

async fn validate_workflow_definition_with_catalog_and_known_subflows<
    T: DefinitionStore + RuntimeStore + FunctionStore,
>(
    db: &T,
    workflow: &WorkflowDefinition,
    known_subflows: &HashSet<Uuid>,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = validate_workflow_definition(workflow)?;
    let providers = catalog::fetch_catalog_items(db, Some("provider_metadata".into())).await?;
    let providers = provider_metadata_from_items(providers)?;
    // type-check `config.*` references against the stored settings schema.
    let config_type = crate::settings::config_type_tree(db).await;
    runinator_workflows::validate_workflow_with_config(&workflow, &providers, &config_type)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    validate_workflow_subflows(db, &workflow, known_subflows).await?;
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
        if export.version_id != binding.version_id {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', but its export does not belong to pinned version {}",
                binding.call_path(),
                binding.version_id
            )));
        }
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
        if version.package_id != binding.package_id {
            return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                "node '{node_id}' calls '{}', but its version does not belong to pinned package {}",
                binding.call_path(),
                binding.package_id
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

async fn validate_workflow_subflows<T: RuntimeStore + DefinitionStore>(
    db: &T,
    workflow: &WorkflowDefinition,
    known_subflows: &HashSet<Uuid>,
) -> Result<(), SendableError> {
    for node in &workflow.definition.nodes {
        if node.kind != WorkflowNodeKind::Subflow {
            continue;
        }
        if let Some(reference) = &node.subflow.target
            && reference.kind != ArtifactKind::Workflow
        {
            return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                "node '{}' references a {:?}, not a workflow",
                node.id, reference.kind
            )));
        }
        let subflow_id = node.subflow.target_workflow_id().or(node.subflow_id);
        if let (Some(reference), Some(legacy_id)) = (&node.subflow.target, node.subflow_id)
            && reference.id != legacy_id
        {
            return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                "node '{}' has conflicting subflow ids {} and {legacy_id}",
                node.id, reference.id
            )));
        }
        if let Some(subflow_id) = subflow_id
            && !subflow_id.is_nil()
            && !known_subflows.contains(&subflow_id)
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
        if let Some(reference) = &node.subflow.target
            && let Some(pin) = &reference.revision_pin
        {
            if pin.revision < 1 || pin.digest.trim().is_empty() {
                return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                    "node '{}' has an incomplete revision pin for workflow {}",
                    node.id, reference.id
                )));
            }
            let revision = db
                .fetch_workflow_revision(reference.id, pin.revision)
                .await?
                .ok_or_else(|| {
                    runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                        "node '{}' pins missing workflow {} revision {}",
                        node.id, reference.id, pin.revision
                    ))
                })?;
            if revision.digest != pin.digest {
                return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                    "node '{}' pins a digest that does not match workflow {} revision {}",
                    node.id, reference.id, pin.revision
                )));
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
    // Assign/locate each logical UUID and resolve every path while the entire pack is in memory.
    // This is the temporary-reference phase: a pair of brand-new, mutually-recursive workflows is
    // never persisted with a name edge merely because its peer has not been saved yet.
    let explicit_ids: HashSet<Uuid> = bundle
        .workflows
        .iter()
        .filter_map(|workflow| workflow.id)
        .collect();
    let workflows = prepare_workflows_for_save(db, bundle.workflows).await?;
    let known_subflows: HashSet<Uuid> = workflows
        .iter()
        .filter_map(|workflow| workflow.id)
        .collect();
    let mut bundle = WorkflowBundle {
        workflows,
        triggers: bundle.triggers,
    };
    // Likewise resolve chaining declarations before any workflow is stored. The authored path is
    // retained for diagnostics, but subsequent rename/move operations cannot break the UUID edge.
    resolve_chained_targets(db, &mut bundle).await?;

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
            && !explicit_ids.contains(&workflow.id.expect("prepared workflow has an id"))
            && let Some(existing) = db
                .fetch_workflow(workflow.id.expect("prepared workflow has an id"))
                .await?
            && !incoming_is_newer(workflow.updated_at, existing.updated_at)
            && workflow_has_resolved_subflow_references(&existing)
        {
            log::info!(
                "Skipping import of workflow '{}': stored copy is up to date",
                workflow.name
            );
            workflows.push(existing);
            continue;
        }
        let imported =
            upsert_prepared_workflow(db, &workflow, &known_subflows, &pack_author).await?;
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

/// Resolve all authored subflow paths to durable artifact references before a save/import.
///
/// Existing rows contribute their current namespace mappings; incoming definitions receive their
/// UUID first and then contribute to the same index. A bare path is accepted for compatibility only
/// when it identifies exactly one workflow. New strict-namespace clients should emit a qualified
/// path, while persisted edges no longer depend on either spelling.
async fn prepare_workflows_for_save<T: DefinitionStore + RuntimeStore>(
    db: &T,
    mut incoming: Vec<WorkflowDefinition>,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    let stored = db.fetch_workflows().await?;
    let mut stored_identities = HashMap::<String, Vec<Uuid>>::new();
    let mut index = HashMap::<String, Vec<Uuid>>::new();
    for workflow in stored {
        let Some(id) = workflow.id else {
            continue;
        };
        let identity = workflow_identity(&workflow);
        let ids = stored_identities.entry(identity).or_default();
        if !ids.contains(&id) {
            ids.push(id);
        }
        add_workflow_aliases(&mut index, &workflow, id);
    }
    let mut seen = HashSet::new();
    for workflow in &incoming {
        let identity = workflow_identity(workflow);
        if !seen.insert(identity.clone()) {
            return Err(
                crate::errors::IMPORT_AMBIGUOUS_ARTIFACT_REFERENCE.error(format!(
                    "pack contains more than one workflow at '{identity}'"
                )),
            );
        }
    }
    for workflow in &mut incoming {
        if workflow.id.is_none() {
            let identity = workflow_identity(workflow);
            workflow.id = match stored_identities.get(&identity).map(Vec::as_slice) {
                Some([id]) => Some(*id),
                Some(ids) if ids.len() > 1 => {
                    return Err(
                        crate::errors::IMPORT_AMBIGUOUS_ARTIFACT_REFERENCE.error(format!(
                            "workflow identity '{identity}' maps to {} existing UUIDs",
                            ids.len()
                        )),
                    );
                }
                _ => Some(Uuid::new_v4()),
            };
        }
    }
    for workflow in &incoming {
        let id = workflow
            .id
            .expect("workflow ids are assigned before resolution");
        let identity = workflow_identity(workflow);
        if let Some(existing) = stored_identities.get(&identity)
            && !existing.contains(&id)
        {
            return Err(
                crate::errors::IMPORT_AMBIGUOUS_ARTIFACT_REFERENCE.error(format!(
                    "workflow identity '{identity}' is already owned by a different UUID"
                )),
            );
        }
        add_workflow_aliases(&mut index, workflow, id);
    }
    let mut prepared = prepare_workflows_against(incoming, index)?;
    resolve_requested_subflow_revisions(db, &mut prepared).await?;
    resolve_setting_bindings(db, &mut prepared).await?;
    Ok(prepared)
}

async fn resolve_setting_bindings<T: RuntimeStore>(
    db: &T,
    workflows: &mut [WorkflowDefinition],
) -> Result<(), SendableError> {
    for workflow in workflows {
        let mut paths = std::collections::BTreeSet::new();
        let graph = serde_json::to_value(&workflow.definition)?;
        collect_setting_paths(&graph, &mut paths);

        let previous = workflow
            .definition
            .metadata
            .pointer("/artifact_refs/settings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|value| serde_json::from_value::<SettingBinding>(value.clone().into()).ok())
            .collect::<Vec<_>>();
        let mut bindings = Vec::new();
        let mut secret_ids = BTreeMap::new();
        for (kind, scope, name) in paths {
            let authored_path = ArtifactPath::new(Some(scope.clone()), name.clone());
            let record = match db.fetch_setting(kind, scope.clone(), name.clone()).await? {
                Some(record) => Some(record),
                None => {
                    let prior = previous.iter().find(|binding| {
                        binding.kind == kind
                            && binding.reference.authored_path.as_ref() == Some(&authored_path)
                    });
                    match prior {
                        Some(prior) => db.fetch_setting_by_id(prior.reference.id).await?,
                        None => None,
                    }
                }
            };
            if let Some(record) = record {
                if kind == SettingKind::Secret {
                    secret_ids.insert((scope.clone(), name.clone()), record.id);
                }
                bindings.push(SettingBinding {
                    kind,
                    reference: ArtifactRef::current(
                        ArtifactKind::Setting,
                        record.id,
                        Some(authored_path),
                    ),
                });
            }
        }
        // Secret references cross the worker boundary, so carry the UUID in the executable value.
        // The authored scope/name stays in the URI for diagnostics and lossless decompilation.
        let mut graph = workflow.definition.as_value();
        pin_secret_refs(&mut graph, &secret_ids);
        workflow.definition = WorkflowGraph::from_value(graph).map_err(invalid_definition)?;
        if workflow.definition.metadata.is_null() {
            workflow.definition.metadata = Value::Object(Default::default());
        }
        let metadata = workflow
            .definition
            .metadata
            .as_object_mut()
            .ok_or_else(|| invalid_definition("workflow metadata must be an object"))?;
        let refs = metadata
            .entry("artifact_refs")
            .or_insert_with(|| Value::Object(Default::default()));
        let refs = refs
            .as_object_mut()
            .ok_or_else(|| invalid_definition("metadata.artifact_refs must be an object"))?;
        refs.insert(
            "settings".into(),
            serde_json::to_value(bindings)
                .map(Value::from)
                .unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }
    Ok(())
}

fn collect_setting_paths(
    value: &serde_json::Value,
    paths: &mut std::collections::BTreeSet<(SettingKind, String, String)>,
) {
    match value {
        serde_json::Value::String(text) => {
            if let Some((scope, name)) = secret_authored_path(text) {
                paths.insert((SettingKind::Secret, scope.to_string(), name.to_string()));
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_setting_paths(value, paths);
            }
        }
        serde_json::Value::Object(object) => {
            if let Some(parts) = object
                .get("$ref")
                .and_then(serde_json::Value::as_object)
                .and_then(|reference| reference.get("config"))
                .and_then(serde_json::Value::as_array)
                && let (Some(scope), Some(name)) = (
                    parts.first().and_then(serde_json::Value::as_str),
                    parts.get(1).and_then(serde_json::Value::as_str),
                )
            {
                paths.insert((SettingKind::Config, scope.to_string(), name.to_string()));
            }
            for (key, value) in object {
                if key != "artifact_refs" {
                    collect_setting_paths(value, paths);
                }
            }
        }
        _ => {}
    }
}

fn secret_authored_path(text: &str) -> Option<(&str, &str)> {
    let rest = if let Some(rest) = text.strip_prefix("secret+uuid://") {
        let (id, path) = rest.split_once('/')?;
        Uuid::parse_str(id).ok()?;
        path
    } else {
        text.strip_prefix("secret://")?
    };
    let (scope, name) = rest.split_once('/')?;
    (!scope.is_empty() && !name.is_empty()).then_some((scope, name))
}

fn pin_secret_refs(value: &mut Value, ids: &BTreeMap<(String, String), Uuid>) {
    match value {
        Value::String(text) => {
            if !text.starts_with("secret+uuid://")
                && let Some((scope, name)) = secret_authored_path(text)
                && let Some(id) = ids.get(&(scope.to_string(), name.to_string()))
            {
                *text = format!("secret+uuid://{id}/{scope}/{name}");
            }
        }
        Value::Array(values) => {
            for value in values {
                pin_secret_refs(value, ids);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key != "artifact_refs" {
                    pin_secret_refs(value, ids);
                }
            }
        }
        _ => {}
    }
}

/// Convert an REXRAP temporary `revision: N` selector into the immutable pin stored in the
/// ArtifactRef. A new target in the same pack has no immutable history yet, so pinning it is
/// rejected rather than silently treating its mutable head as revision N.
async fn resolve_requested_subflow_revisions<T: DefinitionStore>(
    db: &T,
    workflows: &mut [WorkflowDefinition],
) -> Result<(), SendableError> {
    for workflow in workflows {
        for node in &mut workflow.definition.nodes {
            if node.kind != WorkflowNodeKind::Subflow {
                continue;
            }
            let Some(requested_revision) = node.subflow.revision.take() else {
                continue;
            };
            let reference = node.subflow.target.as_mut().ok_or_else(|| {
                crate::errors::IMPORT_UNKNOWN_SUBFLOW.error(format!(
                    "workflow '{}' pins revision {requested_revision} without a subflow target",
                    workflow.name
                ))
            })?;
            if reference.revision_pin.is_some() {
                return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                    "node '{}' carries both a revision selector and a resolved revision pin",
                    node.id
                )));
            }
            let revision = db
                .fetch_workflow_revision(reference.id, requested_revision)
                .await?
                .ok_or_else(|| {
                    runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                        "node '{}' pins missing workflow {} revision {requested_revision}",
                        node.id, reference.id
                    ))
                })?;
            if revision.digest.trim().is_empty() {
                return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                    "workflow {} revision {requested_revision} has no integrity digest",
                    reference.id
                )));
            }
            reference.revision_pin = Some(runinator_models::artifacts::ArtifactRevisionPin {
                revision: requested_revision,
                digest: revision.digest,
            });
        }
    }
    Ok(())
}

fn prepare_workflows_against(
    mut incoming: Vec<WorkflowDefinition>,
    index: HashMap<String, Vec<Uuid>>,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    for workflow in &mut incoming {
        for node in &mut workflow.definition.nodes {
            if node.kind != WorkflowNodeKind::Subflow {
                continue;
            }
            let authored_path = node.subflow.authored_path();
            if let Some(reference) = &mut node.subflow.target {
                if reference.kind != ArtifactKind::Workflow {
                    return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                        "node '{}' references a {:?}, not a workflow",
                        node.id, reference.kind
                    )));
                }
                if reference.authored_path.is_none() {
                    reference.authored_path = authored_path;
                }
                if let Some(legacy_id) = node.subflow_id
                    && legacy_id != reference.id
                {
                    return Err(runinator_runtime::errors::SUBFLOW_INVALID_ID.error(format!(
                        "node '{}' has conflicting subflow ids {} and {legacy_id}",
                        node.id, reference.id
                    )));
                }
                node.subflow_id = Some(reference.id);
                continue;
            }
            if let Some(id) = node.subflow_id.filter(|id| !id.is_nil()) {
                node.subflow.target = Some(ArtifactRef::current(
                    ArtifactKind::Workflow,
                    id,
                    authored_path,
                ));
                continue;
            }
            let Some(path) = authored_path else {
                continue;
            };
            let target = resolve_workflow_path(&index, &path, &workflow.name)?;
            node.subflow_id = Some(target);
            node.subflow.target = Some(ArtifactRef::current(
                ArtifactKind::Workflow,
                target,
                Some(path),
            ));
        }
    }
    Ok(incoming)
}

fn workflow_identity(workflow: &WorkflowDefinition) -> String {
    workflow.artifact_path().qualified()
}

fn add_workflow_aliases(
    index: &mut HashMap<String, Vec<Uuid>>,
    workflow: &WorkflowDefinition,
    id: Uuid,
) {
    let display_path =
        ArtifactPath::new(workflow.namespace.clone(), workflow.name.clone()).qualified();
    for alias in [
        workflow_identity(workflow),
        display_path,
        workflow.name.clone(),
    ] {
        let candidates = index.entry(alias).or_default();
        if !candidates.contains(&id) {
            candidates.push(id);
        }
    }
}

fn resolve_workflow_path(
    index: &HashMap<String, Vec<Uuid>>,
    path: &ArtifactPath,
    owner: &str,
) -> Result<Uuid, SendableError> {
    match index.get(&path.qualified()).map(Vec::as_slice) {
        Some([id]) => Ok(*id),
        Some(ids) if ids.len() > 1 => Err(crate::errors::IMPORT_AMBIGUOUS_ARTIFACT_REFERENCE
            .error(format!(
                "workflow '{owner}' references '{}', which maps to {} workflow UUIDs",
                path,
                ids.len()
            ))),
        _ => Err(crate::errors::IMPORT_UNKNOWN_SUBFLOW.error(format!(
            "workflow '{owner}' references unknown subflow workflow '{path}'"
        ))),
    }
}

fn workflow_has_resolved_subflow_references(workflow: &WorkflowDefinition) -> bool {
    workflow.definition.nodes.iter().all(|node| {
        node.kind != WorkflowNodeKind::Subflow || node.subflow.target_workflow_id().is_some()
    })
}

/// Resolve every chained trigger to the workflow UUID present in this bundle or already stored.
async fn resolve_chained_targets<T: DefinitionStore + RuntimeStore>(
    db: &T,
    bundle: &mut WorkflowBundle,
) -> Result<(), SendableError> {
    let mut index = HashMap::<String, Vec<Uuid>>::new();
    for workflow in db.fetch_workflows().await? {
        let Some(id) = workflow.id else { continue };
        add_workflow_aliases(&mut index, &workflow, id);
    }
    for workflow in &bundle.workflows {
        let Some(id) = workflow.id else { continue };
        add_workflow_aliases(&mut index, workflow, id);
    }
    let incoming_ids: HashSet<Uuid> = bundle
        .workflows
        .iter()
        .filter_map(|workflow| workflow.id)
        .collect();
    for workflow in &mut bundle.workflows {
        let Some(specs) = workflow
            .definition
            .metadata
            .pointer_mut("/triggers")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for spec in specs {
            if spec.get("kind").and_then(Value::as_str) != Some("chained") {
                continue;
            }
            if let Some(id) = spec
                .get("target_workflow_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
            {
                if db.fetch_workflow(id).await?.is_some() || incoming_ids.contains(&id) {
                    continue;
                }
                return Err(crate::errors::IMPORT_UNKNOWN_CHAINED_TARGET.error(format!(
                    "workflow '{}' chains to missing UUID {id}",
                    workflow.name
                )));
            }
            let Some(name) = spec
                .get("target_workflow")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let path = ArtifactPath::from_qualified(name);
            let id = resolve_workflow_path(&index, &path, &workflow.name)?;
            let reference = ArtifactRef::current(ArtifactKind::Workflow, id, Some(path));
            let object = spec
                .as_object_mut()
                .expect("trigger declarations lower to objects");
            object.insert("target_workflow_id".into(), Value::String(id.to_string()));
            object.insert(
                "target".into(),
                serde_json::to_value(reference)
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            );
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
                        "target_workflow_id": spec.get("target_workflow_id").cloned(),
                        "target": spec.get("target").cloned(),
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
    // Namespace aliases are deliberately soft, but a UUID reference is not: removing its target
    // would leave a definition that can no longer be executed. Check both the new ArtifactRef and
    // the legacy mirror while older persisted JSON is still being lazily migrated.
    let mut inbound: Vec<_> = db
        .fetch_workflows()
        .await?
        .into_iter()
        .filter(|workflow| workflow.id != Some(workflow_id))
        .filter(|workflow| {
            workflow.definition.nodes.iter().any(|node| {
                node.kind == WorkflowNodeKind::Subflow
                    && (node.subflow.target_workflow_id() == Some(workflow_id)
                        || node.subflow_id == Some(workflow_id))
            }) || workflow
                .definition
                .metadata
                .pointer("/triggers")
                .and_then(Value::as_array)
                .is_some_and(|triggers| {
                    triggers.iter().any(|trigger| {
                        trigger
                            .get("target_workflow_id")
                            .and_then(Value::as_str)
                            .and_then(|value| Uuid::parse_str(value).ok())
                            == Some(workflow_id)
                    })
                })
        })
        .map(|workflow| workflow_identity(&workflow))
        .collect();
    // Pipeline members already persist workflow UUIDs. Treat those as equally hard inbound
    // references so deleting a workflow cannot leave a pipeline graph with a dangling member.
    inbound.extend(
        db.fetch_pipelines()
            .await?
            .into_iter()
            .filter(|pipeline| {
                pipeline
                    .graph
                    .members
                    .iter()
                    .any(|member| member.workflow_id == workflow_id)
            })
            .map(|pipeline| format!("pipeline {}", pipeline.name)),
    );
    if !inbound.is_empty() {
        return Err(
            crate::errors::ARTIFACT_HAS_INBOUND_REFERENCES.error(format!(
                "workflow {workflow_id} is referenced by {}",
                inbound.join(", ")
            )),
        );
    }
    db.delete_workflow(workflow_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow deleted".into(),
    })
}
