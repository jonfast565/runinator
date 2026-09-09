use super::*;

use std::collections::BTreeMap;

use runinator_models::execution_profiles::{
    ExecutionProfileCollectionSpec, ExecutionProfileCollectionStatus, ExecutionProfileExposureSpec,
    ExecutionProfilePutRequest,
};

pub(super) async fn execution_profiles(
    client: &Client,
    command: &ExecutionProfileCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ExecutionProfileCommands::List => {
            let profiles = client.list_execution_profiles().await?;
            if json_output {
                return output::json(&profiles);
            }
            println!("{:<36} {:<24} health", "id", "name");
            for profile in profiles {
                println!(
                    "{:<36} {:<24} {}",
                    profile.id,
                    output::truncate(&profile.name, 24),
                    profile.health.as_str()
                );
            }
        }
        ExecutionProfileCommands::Show { id } => {
            let profile = client.fetch_execution_profile(*id).await?;
            if json_output {
                return output::json(&profile);
            }
            print!("{}", output::value_table(&profile)?);
        }
        ExecutionProfileCommands::Status { id } => {
            let (profiles, mut statuses) = tokio::try_join!(
                client.list_execution_profiles(),
                client.list_execution_profile_collection_statuses(),
            )?;
            if let Some(id) = id {
                statuses.retain(|status| status.profile_id == *id);
                if statuses.is_empty() {
                    return Err(err(format!("execution profile {id} not found")));
                }
            }
            if json_output {
                return output::json(&statuses);
            }
            if statuses.is_empty() {
                println!("no execution profiles are configured");
                return Ok(());
            }
            let names = profiles
                .into_iter()
                .map(|profile| (profile.id, profile.name))
                .collect::<BTreeMap<_, _>>();
            print!("{}", collection_status_text(&statuses, &names));
        }
        ExecutionProfileCommands::Add {
            name,
            description,
            credential_scopes,
            collection,
            exposure,
            id,
        } => {
            let request = profile_request(
                name,
                description.as_deref(),
                credential_scopes,
                collection,
                exposure.as_deref(),
            )?;
            let profile = client
                .configure_execution_profile(id.unwrap_or_else(Uuid::new_v4), &request)
                .await?;
            if json_output {
                return output::json(&profile);
            }
            println!("configured {} ({})", profile.name, profile.id);
        }
        ExecutionProfileCommands::Apply { file, id } => {
            let request =
                serde_json::from_str::<ExecutionProfilePutRequest>(&fs::read_to_string(file)?)?;
            let profile = client
                .configure_execution_profile(id.unwrap_or_else(Uuid::new_v4), &request)
                .await?;
            if json_output {
                return output::json(&profile);
            }
            println!("configured {} ({})", profile.name, profile.id);
        }
        ExecutionProfileCommands::Delete { id } => {
            let result = client.delete_execution_profile(*id).await?;
            if json_output {
                return output::json(&result);
            }
            println!("deleted execution profile {id}");
        }
        ExecutionProfileCommands::Rotate { id } => {
            let result = client.rotate_execution_profile(*id).await?;
            if json_output {
                return output::json(&result);
            }
            println!("requested rotation for execution profile {id}");
        }
        ExecutionProfileCommands::Test { id } => {
            let result = client.test_execution_profile(*id).await?;
            if json_output {
                return output::json(&result);
            }
            println!("requested collection dry run for execution profile {id}");
        }
    }
    Ok(())
}

fn profile_request(
    name: &str,
    description: Option<&str>,
    credential_scopes: &[String],
    collection_json: &str,
    exposure_json: Option<&str>,
) -> Result<ExecutionProfilePutRequest> {
    let collection = serde_json::from_str::<ExecutionProfileCollectionSpec>(collection_json)
        .map_err(|error| {
            err(format!(
                "--collection must be valid profile collection JSON: {error}"
            ))
        })?;
    let exposure = exposure_json
        .map(|json| {
            serde_json::from_str::<ExecutionProfileExposureSpec>(json).map_err(|error| {
                err(format!(
                    "--exposure must be valid profile exposure JSON: {error}"
                ))
            })
        })
        .transpose()?
        .unwrap_or_default();

    Ok(ExecutionProfilePutRequest {
        name: name.to_string(),
        description: description.unwrap_or_default().to_string(),
        credential_scopes: credential_scopes.to_vec(),
        collection,
        exposure,
        enabled: true,
    })
}

fn collection_status_text(
    statuses: &[ExecutionProfileCollectionStatus],
    names: &BTreeMap<Uuid, String>,
) -> String {
    let mut text = String::new();
    for (index, status) in statuses.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        let name = names
            .get(&status.profile_id)
            .map(String::as_str)
            .unwrap_or("unknown profile");
        let revision = status
            .current_revision
            .map(|revision| revision.to_string())
            .unwrap_or_else(|| "none".into());
        let published = status
            .published_at
            .as_ref()
            .map(|published| published.to_rfc3339())
            .unwrap_or_else(|| "never".into());
        let operation = status
            .latest_operation
            .as_ref()
            .map(|operation| {
                let error = operation
                    .error
                    .as_deref()
                    .map(|error| format!(" ({})", output::truncate(error, 80)))
                    .unwrap_or_default();
                format!(
                    "{} {}{}",
                    operation.kind.as_str(),
                    operation.state.as_str(),
                    error
                )
            })
            .unwrap_or_else(|| "none".into());
        text.push_str(&format!(
            "{name} ({})\npublication: {}; revision: {revision}; published: {published}\nlatest operation: {operation}\n",
            status.profile_id,
            status.publication_health.as_str(),
        ));
        let rows = match status.agents.is_empty() {
            true => vec![vec![
                "-".into(),
                "not reported".into(),
                "-".into(),
                "-".into(),
                "no desktop agent has reported this profile".into(),
            ]],
            false => status
                .agents
                .iter()
                .map(|agent| {
                    vec![
                        agent.agent_id.to_string(),
                        agent.approval.as_str().into(),
                        agent.last_seen_at.to_rfc3339(),
                        agent
                            .last_success_at
                            .as_ref()
                            .map(|success| success.to_rfc3339())
                            .unwrap_or_else(|| "-".into()),
                        agent
                            .last_error
                            .as_deref()
                            .map(|error| output::truncate(error, 80))
                            .unwrap_or_else(|| "-".into()),
                    ]
                })
                .collect(),
        };
        text.push_str(&output::table(
            &[
                "desktop agent",
                "approval",
                "last seen",
                "last success",
                "error",
            ],
            &rows,
        ));
    }
    text
}

#[cfg(test)]
#[path = "execution_profiles_tests.rs"]
mod tests;
