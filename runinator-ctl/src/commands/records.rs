use super::*;

pub(super) async fn records(
    client: &Client,
    command: &RecordCommands,
    json_output: bool,
) -> Result<()> {
    let collection = match command {
        RecordCommands::ExternalItems => "external_items",
        RecordCommands::Events => "automation_events",
    };
    let value = client.fetch_record_collection(collection).await?;
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
