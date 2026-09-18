use super::*;

pub(super) async fn billing(
    client: &Client,
    command: &BillingCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        BillingCommands::RateCard => client.fetch_rate_card().await?,
        BillingCommands::UpdateAi { file } => {
            client
                .update_ai_rate_card(params::load_json_file(file)?)
                .await?
        }
    };
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
