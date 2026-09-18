use super::*;

use runinator_ctl_core::cli::{AccountSessionCommands, PersonalKeyCommands};

pub(super) async fn account(
    client: &Client,
    command: &AccountCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        AccountCommands::Show => client.fetch_account().await?,
        AccountCommands::Update { file } => {
            client
                .update_account(&params::load_json_file(file)?)
                .await?
        }
        AccountCommands::Password { file } => {
            client
                .change_account_password(&params::load_json_file(file)?)
                .await?
        }
        AccountCommands::Sessions { command } => match command {
            AccountSessionCommands::List => client.fetch_account_sessions().await?,
            AccountSessionCommands::Revoke { id } => client.revoke_account_session(*id).await?,
            AccountSessionCommands::RevokeOthers => client.revoke_other_account_sessions().await?,
        },
        AccountCommands::Keys { command } => match command {
            PersonalKeyCommands::Scopes => client.fetch_personal_api_key_scopes().await?,
            PersonalKeyCommands::List => client.fetch_personal_api_keys().await?,
            PersonalKeyCommands::Create { file } => {
                client
                    .create_personal_api_key(&params::load_json_file(file)?)
                    .await?
            }
        },
    };
    print_value(&value, json_output)
}

fn print_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(value);
    }
    print!("{}", output::value_table(value)?);
    Ok(())
}
