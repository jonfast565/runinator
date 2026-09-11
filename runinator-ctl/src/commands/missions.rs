//! Narrow, orchestration-backed controls intended for harnessed coding and research missions.

use super::*;

use runinator_ctl_core::cli::MissionCommands;

pub(super) async fn missions(
    client: &Client,
    command: &MissionCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        MissionCommands::Start {
            pipeline,
            kind,
            correlation,
            json_file,
            event_id,
        } => {
            let pipeline = super::pipelines::resolve_pipeline(client, pipeline).await?;
            let pipeline_id = super::pipelines::pipeline_id(&pipeline)?;
            if !pipeline.metadata.get("orchestration").is_some() {
                return Err(err(
                    "missions start requires a managed orchestration pipeline",
                ));
            }
            let scope = pipeline
                .metadata
                .get("ingress")
                .and_then(|ingress| ingress.get("scope"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !scope.starts_with("mission.") {
                return Err(err(
                    "missions start requires a pipeline whose ingress scope starts with 'mission.'",
                ));
            }
            let expected_scope = format!("mission.{}", kind.as_str());
            if scope != expected_scope {
                return Err(err(format!(
                    "missions start kind '{}' requires ingress scope '{expected_scope}', found '{scope}'",
                    kind.as_str(),
                )));
            }
            let mut payload = params::load_json_file(json_file)?;
            let Some(payload) = payload.as_object_mut() else {
                return Err(err("mission input JSON must be an object"));
            };
            let mut mission = payload
                .remove("mission")
                .and_then(|value| match value {
                    Value::Object(object) => Some(object),
                    _ => None,
                })
                .unwrap_or_default();
            validate_mission_input(payload, &mission)?;
            mission.insert("kind".into(), Value::String(kind.as_str().into()));
            mission.insert("correlation_key".into(), Value::String(correlation.clone()));
            mission.insert("requested_by".into(), Value::String("runinatorctl".into()));
            payload.insert("mission".into(), Value::Object(mission));
            let event_id = event_id
                .clone()
                .unwrap_or_else(|| Uuid::now_v7().to_string());
            let response = client
                .ingress_pipeline(
                    pipeline_id,
                    &PipelineIngressRequest {
                        source: "runinator.mission".into(),
                        event_id: event_id.clone(),
                        event_type: "start".into(),
                        correlation_key: correlation.clone(),
                        payload: Value::Object(payload.clone()),
                        provenance: json!({
                            "origin": "runinatorctl",
                            "mission_kind": kind.as_str(),
                        }),
                    },
                )
                .await?;
            if json_output {
                return output::json(&response);
            }
            let binding = response
                .orchestration_binding_id
                .as_deref()
                .unwrap_or("pending reducer admission");
            println!(
                "started {} mission {} [{}]",
                kind.as_str(),
                binding,
                event_id
            );
            Ok(())
        }
        MissionCommands::List { status, limit } => {
            let mut bindings = client
                .fetch_orchestrations_filtered(OrchestrationListQuery {
                    status: status.as_deref(),
                    limit: Some(*limit),
                    scope_prefix: Some("mission."),
                    ..Default::default()
                })
                .await?;
            bindings.retain(|binding| binding.scope.starts_with("mission."));
            if json_output {
                return output::json(&bindings);
            }
            let rows = bindings
                .into_iter()
                .map(|binding| {
                    vec![
                        binding.id.to_string(),
                        binding.scope,
                        binding.correlation_key,
                        binding.status.as_str().into(),
                        binding.current_phase.unwrap_or_else(|| "-".into()),
                        binding.current_epoch.to_string(),
                        output::time(Some(binding.updated_at)),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(
                    &[
                        "ID", "SCOPE", "MISSION", "STATUS", "PHASE", "EPOCH", "UPDATED"
                    ],
                    &rows,
                )
            );
            Ok(())
        }
        MissionCommands::Show { id } => {
            let binding = mission_binding(client, *id).await?;
            if json_output {
                return output::json(&binding);
            }
            print!("{}", output::value_table(&binding)?);
            Ok(())
        }
        MissionCommands::Evidence { id } => {
            mission_binding(client, *id).await?;
            let evidence = client.fetch_orchestration_evidence(*id).await?;
            if json_output {
                return output::json(&evidence);
            }
            let rows = evidence
                .into_iter()
                .map(|entry| {
                    vec![
                        entry.id.to_string(),
                        entry
                            .epoch
                            .map_or_else(|| "-".into(), |epoch| epoch.to_string()),
                        entry.kind,
                        entry.subject_revision.unwrap_or_else(|| "-".into()),
                        output::time(Some(entry.created_at)),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(&["ID", "EPOCH", "KIND", "REVISION", "CREATED"], &rows)
            );
            Ok(())
        }
        MissionCommands::Steer { id, message } => {
            mission_binding(client, *id).await?;
            let response = client.steer_mission(*id, message).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
        MissionCommands::Intent {
            id,
            name,
            reason,
            payload,
            idempotency_key,
        } => {
            mission_binding(client, *id).await?;
            let payload = payload
                .as_deref()
                .map(params::load_json_file)
                .transpose()?
                .unwrap_or_default();
            let key = idempotency_key
                .clone()
                .unwrap_or_else(|| Uuid::now_v7().to_string());
            let response = client
                .send_orchestration_intent(*id, name, payload, reason, &key)
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!("accepted mission intent {name} [{key}]");
            Ok(())
        }
    }
}

fn validate_mission_input(
    payload: &runinator_models::value::Map,
    mission: &runinator_models::value::Map,
) -> Result<()> {
    let goal = payload
        .get("request")
        .and_then(Value::as_object)
        .and_then(|request| request.get("goal"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if goal.is_empty() {
        return Err(err("mission input requires a non-empty request.goal"));
    }
    let source = mission.get("source").and_then(Value::as_object);
    for field in ["repository", "revision"] {
        let value = source
            .and_then(|source| source.get(field))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if value.is_empty() {
            return Err(err(format!(
                "mission input requires a non-empty mission.source.{field}"
            )));
        }
    }
    Ok(())
}

async fn mission_binding(
    client: &Client,
    id: Uuid,
) -> Result<runinator_models::orchestration::OrchestrationBinding> {
    let binding = client.fetch_orchestration(id).await?;
    if !binding.scope.starts_with("mission.") {
        return Err(err(format!("orchestration {id} is not a mission")));
    }
    Ok(binding)
}

#[cfg(test)]
#[path = "missions_tests.rs"]
mod tests;
