use super::*;

pub(super) async fn providers(
    client: &Client,
    command: &ProviderCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ProviderCommands::List => {
            let providers = client.fetch_providers().await?;
            if json_output {
                return output::json(&providers);
            }
            print_providers(&providers);
        }
        ProviderCommands::Show { name } => {
            let providers = client.fetch_providers().await?;
            let Some(provider) = providers
                .into_iter()
                .find(|provider| provider.name == *name)
            else {
                return Err(err(format!("provider '{name}' not found")));
            };
            if json_output {
                return output::json(&provider);
            }
            print_provider(&provider);
        }
    }
    Ok(())
}
