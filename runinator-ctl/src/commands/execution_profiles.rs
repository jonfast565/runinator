use super::*;

use runinator_models::execution_profiles::{
    ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec, ExecutionProfilePutRequest,
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
            output::json(&profile)?;
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

#[cfg(test)]
#[path = "execution_profiles_tests.rs"]
mod tests;
