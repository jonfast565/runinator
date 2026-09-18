use super::*;

pub(super) async fn catalog(
    client: &Client,
    command: &CatalogCommands,
    json_output: bool,
) -> Result<()> {
    let path = match command {
        CatalogCommands::NodeKinds => "/node-kinds",
        CatalogCommands::TriggerKinds => "/trigger-kinds",
        CatalogCommands::Enums => "/catalog/enums",
    };
    let value = client.fetch_catalog_metadata(path).await?;
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
