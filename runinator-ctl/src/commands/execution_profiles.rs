use super::*;

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
        ExecutionProfileCommands::Apply { file, id } => {
            let request = serde_json::from_str::<
                runinator_models::execution_profiles::ExecutionProfilePutRequest,
            >(&fs::read_to_string(file)?)?;
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
