use super::*;

use std::collections::{BTreeMap, BTreeSet};

use runinator_models::{
    orchestration::{AdapterAuthentication, AdapterKindMetadata, AdapterTransport, IngressPolicy},
    settings::SettingKind,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct DoctorFinding {
    status: &'static str,
    check: &'static str,
    resource: String,
    message: String,
    remediation: Option<String>,
}

impl DoctorFinding {
    fn error(
        check: &'static str,
        resource: impl Into<String>,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            status: "error",
            check,
            resource: resource.into(),
            message: message.into(),
            remediation: Some(remediation.into()),
        }
    }
}

pub(super) async fn doctor(client: &Client, json_output: bool) -> Result<()> {
    let kinds = client.fetch_orchestration_adapter_kinds().await?;
    let adapters = client.fetch_orchestration_adapters().await?;
    let profiles = client.list_execution_profiles().await?;
    let settings = client.list_settings().await?;
    let workflows = client.fetch_workflows().await?;
    let pipelines = client.fetch_pipelines().await?;
    let metadata = kinds
        .iter()
        .filter(|entry| entry.healthy)
        .map(|entry| entry.metadata.clone())
        .collect::<Vec<_>>();
    let kinds_by_name = kinds
        .iter()
        .map(|entry| (entry.metadata.kind.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let profile_by_id = profiles
        .iter()
        .map(|profile| (profile.id, profile))
        .collect::<BTreeMap<_, _>>();
    let setting_ids = settings
        .iter()
        .map(|setting| setting.id)
        .collect::<BTreeSet<_>>();
    let setting_paths = settings
        .iter()
        .map(|setting| (setting.kind, setting.scope.clone(), setting.name.clone()))
        .collect::<BTreeSet<_>>();
    let mut findings = Vec::new();

    for workflow in &workflows {
        inspect_ingress(
            &workflow.definition.metadata,
            &metadata,
            "workflow",
            &workflow.name,
            &mut findings,
        );
        inspect_setting_paths(
            &workflow.definition.as_value(),
            &setting_paths,
            &format!("workflow {}", workflow.name),
            &mut findings,
        );
    }
    for pipeline in &pipelines {
        inspect_ingress(
            &pipeline.metadata,
            &metadata,
            "pipeline",
            &pipeline.name,
            &mut findings,
        );
    }

    for adapter in &adapters {
        let resource = format!("adapter {} ({})", adapter.name, adapter.id);
        let Some(kind) = kinds_by_name.get(adapter.kind.as_str()) else {
            findings.push(DoctorFinding::error(
                "adapter-kind",
                &resource,
                format!("adapter kind '{}' is not installed", adapter.kind),
                "install or restore the adapter kind, then reload the adapter host",
            ));
            continue;
        };
        if !kind.healthy {
            findings.push(DoctorFinding::error(
                "adapter-kind",
                &resource,
                kind.error
                    .clone()
                    .unwrap_or_else(|| format!("adapter kind '{}' is unhealthy", adapter.kind)),
                "repair the adapter plugin and run `runinatorctl orchestrations adapters reload`",
            ));
            continue;
        }
        let revisions = client
            .fetch_orchestration_adapter_revisions(adapter.id)
            .await?;
        let Some(revision) = revisions
            .iter()
            .find(|revision| revision.revision == adapter.current_revision)
        else {
            findings.push(DoctorFinding::error(
                "adapter-revision",
                &resource,
                format!("current revision {} is missing", adapter.current_revision),
                "re-apply the adapter definition",
            ));
            continue;
        };
        if revision.kind_version != kind.metadata.version {
            findings.push(DoctorFinding::error(
                "adapter-version",
                &resource,
                format!(
                    "revision pins kind version {}, but the adapter host loads {}",
                    revision.kind_version, kind.metadata.version
                ),
                "re-apply the adapter against the loaded kind version or restore the pinned plugin",
            ));
        }
        inspect_adapter_configuration(
            &resource,
            revision.transport,
            &revision.configuration,
            &kind.metadata,
            &mut findings,
        );
        match &revision.authentication {
            AdapterAuthentication::Secrets { secret_bindings } => {
                for (name, id) in secret_bindings {
                    if !setting_ids.contains(id) {
                        findings.push(DoctorFinding::error(
                            "adapter-secret",
                            &resource,
                            format!("secret binding '{name}' points to missing setting {id}"),
                            "store the secret and re-apply the adapter with its setting id",
                        ));
                    }
                }
            }
            AdapterAuthentication::ExecutionProfile {
                profile,
                required_scopes,
                ..
            } => match profile_by_id.get(&profile.id()) {
                None => findings.push(DoctorFinding::error(
                    "execution-profile",
                    &resource,
                    format!(
                        "execution profile '{}' ({}) is not readable in the active scope",
                        profile.name(),
                        profile.id()
                    ),
                    "grant the adapter access to an organization or platform profile and re-apply it",
                )),
                Some(resolved) => {
                    let missing = required_scopes
                        .iter()
                        .filter(|scope| !resolved.credential_scopes.contains(scope))
                        .cloned()
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        findings.push(DoctorFinding::error(
                            "execution-profile",
                            &resource,
                            format!("execution profile is missing scopes: {}", missing.join(", ")),
                            "update or replace the profile with the required credential scopes",
                        ));
                    }
                    if !resolved.enabled
                        || resolved.current_revision.is_none()
                        || !matches!(
                            resolved.health,
                            runinator_models::execution_profiles::ExecutionProfileHealth::Ready
                                | runinator_models::execution_profiles::ExecutionProfileHealth::Expiring
                        )
                    {
                        findings.push(DoctorFinding::error(
                            "execution-profile",
                            &resource,
                            format!(
                                "execution profile '{}' is {} and has published revision {:?}",
                                resolved.name,
                                resolved.health.as_str(),
                                resolved.current_revision
                            ),
                            "approve, collect, and publish the profile from a desktop agent",
                        ));
                    }
                }
            },
        }
        if revision.transport == AdapterTransport::Polling {
            let status = client
                .fetch_orchestration_adapter_poll_status(adapter.id)
                .await?;
            if let Some(diagnostic) = status.worker_diagnostic {
                findings.push(DoctorFinding::error(
                    "worker-labels",
                    &resource,
                    diagnostic,
                    "start a live worker with every required label or change the adapter binding",
                ));
            }
            if let Some(error) = status.last_error {
                findings.push(DoctorFinding::error(
                    "polling",
                    &resource,
                    error,
                    "inspect `orchestrations adapters attempts` and repair the reported failure",
                ));
            }
            for attempt in client
                .fetch_orchestration_adapter_attempts(adapter.id)
                .await?
                .into_iter()
                .take(1)
            {
                if attempt.error.as_deref().is_some_and(|error| {
                    error.contains("adapter host implements") && error.contains("pinned version")
                }) {
                    findings.push(DoctorFinding::error(
                        "adapter-version",
                        &resource,
                        attempt.error.unwrap_or_default(),
                        "deploy the matching adapter host or re-apply the adapter revision",
                    ));
                }
            }
        }
    }

    let error_count = findings
        .iter()
        .filter(|finding| finding.status == "error")
        .count();
    let warning_count = findings
        .iter()
        .filter(|finding| finding.status == "warning")
        .count();
    if json_output {
        return output::json(&json!({
            "healthy": error_count == 0,
            "errors": error_count,
            "warnings": warning_count,
            "findings": findings,
        }));
    }
    if findings.is_empty() {
        println!("doctor found no harness wiring problems");
        return Ok(());
    }
    let rows = findings
        .iter()
        .map(|finding| {
            vec![
                finding.status.to_uppercase(),
                finding.check.into(),
                finding.resource.clone(),
                finding.message.clone(),
                finding.remediation.clone().unwrap_or_default(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &["STATUS", "CHECK", "RESOURCE", "DETAIL", "NEXT STEP"],
            &rows,
        )
    );
    println!("doctor found {error_count} error(s) and {warning_count} warning(s)");
    Ok(())
}

fn inspect_ingress(
    metadata: &Value,
    kinds: &[AdapterKindMetadata],
    kind: &'static str,
    name: &str,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(value) = metadata.get("ingress") else {
        return;
    };
    let Ok(policy) = serde_json::from_value::<IngressPolicy>(value.clone().into()) else {
        findings.push(DoctorFinding::error(
            "scope-reachability",
            format!("{kind} {name}"),
            "ingress metadata is not a valid policy",
            "re-apply the source after `runinatorctl rexrap check` succeeds",
        ));
        return;
    };
    if let Err(error) = policy.validate_reachability(kinds) {
        findings.push(DoctorFinding::error(
            "scope-reachability",
            format!("{kind} {name}"),
            error,
            "change the ingress scope to one emitted by an installed adapter kind",
        ));
    }
}

fn inspect_adapter_configuration(
    resource: &str,
    transport: AdapterTransport,
    configuration: &Value,
    kind: &AdapterKindMetadata,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(configuration) = configuration.as_object() else {
        findings.push(DoctorFinding::error(
            "adapter-configuration",
            resource,
            "configuration is not an object",
            "re-apply the adapter using its current typed schema",
        ));
        return;
    };
    let fields = if transport == AdapterTransport::Polling {
        &kind.polling_fields
    } else {
        &kind.fields
    };
    let declared = fields
        .iter()
        .chain(
            (transport == AdapterTransport::Polling)
                .then_some(kind.polling_secret_fields.iter())
                .into_iter()
                .flatten(),
        )
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    for key in configuration
        .keys()
        .filter(|key| !declared.contains(key.as_str()))
    {
        findings.push(DoctorFinding::error(
            "adapter-configuration",
            resource,
            format!(
                "configuration key '{key}' is inert because adapter kind '{}' does not declare it",
                kind.kind
            ),
            "remove the key and re-apply the adapter using a declared configuration field",
        ));
    }
}

fn inspect_setting_paths(
    graph: &Value,
    available: &BTreeSet<(SettingKind, String, String)>,
    resource: &str,
    findings: &mut Vec<DoctorFinding>,
) {
    let mut referenced = BTreeSet::new();
    collect_setting_paths(&serde_json::Value::from(graph.clone()), &mut referenced);
    for (kind, scope, name) in referenced.difference(available) {
        findings.push(DoctorFinding::error(
            "setting",
            resource,
            format!("{} {scope}/{name} is not provisioned", kind.as_str()),
            format!(
                "run `runinatorctl settings set {scope} {name} VALUE --kind {}`",
                kind.as_str()
            ),
        ));
    }
}

fn collect_setting_paths(
    value: &serde_json::Value,
    paths: &mut BTreeSet<(SettingKind, String, String)>,
) {
    match value {
        serde_json::Value::String(text) => {
            let path = text.strip_prefix("secret://").or_else(|| {
                text.strip_prefix("secret+uuid://")
                    .and_then(|rest| rest.split_once('/').map(|(_, path)| path))
            });
            if let Some((scope, name)) = path.and_then(|path| path.split_once('/'))
                && !scope.is_empty()
                && !name.is_empty()
            {
                paths.insert((SettingKind::Secret, scope.into(), name.into()));
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
                paths.insert((SettingKind::Config, scope.into(), name.into()));
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
