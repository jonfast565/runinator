use super::*;

use runinator_ctl_core::cli::{
    AdminApiKeyCommands, AdminTeamCommands, AdminUserCommands, PolicyCommands,
    RuntimePolicyCommands,
};

pub(super) async fn admin(
    client: &Client,
    command: &AdminCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        AdminCommands::Users { command } => users(client, command).await?,
        AdminCommands::Teams { command } => teams(client, command).await?,
        AdminCommands::ApiKeys { command } => api_keys(client, command).await?,
        AdminCommands::AuthSettings { command } => {
            policy(client, command, "/auth/settings").await?
        }
        AdminCommands::ServerSettings { command } => {
            policy(client, command, "/server/settings").await?
        }
        AdminCommands::Runtimes { command } => runtimes(client, command).await?,
    };
    print_value(&value, json_output)
}

async fn users(client: &Client, command: &AdminUserCommands) -> Result<Value> {
    Ok(match command {
        AdminUserCommands::List => client.fetch_admin_collection("users").await?,
        AdminUserCommands::Create { file } => {
            client
                .create_admin_resource("users", &params::load_json_file(file)?)
                .await?
        }
        AdminUserCommands::Update { id, file } => {
            client
                .update_admin_resource("users", *id, &params::load_json_file(file)?)
                .await?
        }
        AdminUserCommands::Delete { id } => client.delete_admin_resource("users", *id).await?,
        AdminUserCommands::Teams { id } => client.fetch_user_teams(*id).await?,
    })
}

async fn teams(client: &Client, command: &AdminTeamCommands) -> Result<Value> {
    Ok(match command {
        AdminTeamCommands::List => client.fetch_admin_collection("teams").await?,
        AdminTeamCommands::Create { name } => {
            client
                .create_admin_resource("teams", &json!({ "name": name }))
                .await?
        }
        AdminTeamCommands::Update { id, name } => {
            client
                .update_admin_resource("teams", *id, &json!({ "name": name }))
                .await?
        }
        AdminTeamCommands::Delete { id } => client.delete_admin_resource("teams", *id).await?,
        AdminTeamCommands::Members { id } => client.fetch_team_members(*id).await?,
        AdminTeamCommands::AddMember { id, user, role } => {
            client.add_team_member(*id, *user, role).await?
        }
        AdminTeamCommands::RemoveMember { id, user } => {
            client.remove_team_member(*id, *user).await?
        }
    })
}

async fn api_keys(client: &Client, command: &AdminApiKeyCommands) -> Result<Value> {
    Ok(match command {
        AdminApiKeyCommands::List => client.fetch_admin_collection("api_keys").await?,
        AdminApiKeyCommands::Create { file } => {
            client
                .create_admin_resource("api_keys", &params::load_json_file(file)?)
                .await?
        }
        AdminApiKeyCommands::Update { id, file } => {
            client
                .update_admin_resource("api_keys", *id, &params::load_json_file(file)?)
                .await?
        }
        AdminApiKeyCommands::Rotate { id } => client.rotate_admin_api_key(*id).await?,
        AdminApiKeyCommands::Revoke { id } => client.delete_admin_resource("api_keys", *id).await?,
    })
}

async fn policy(client: &Client, command: &PolicyCommands, path: &str) -> Result<Value> {
    match command {
        PolicyCommands::Show => client.fetch_policy(path).await.map_err(Into::into),
        PolicyCommands::Apply { file } => client
            .apply_policy(path, &params::load_json_file(file)?)
            .await
            .map_err(Into::into),
    }
}

async fn runtimes(client: &Client, command: &RuntimePolicyCommands) -> Result<Value> {
    match command {
        RuntimePolicyCommands::Show { language } => client
            .get_setting(SettingKind::Config, "foreign_languages", language)
            .await
            .map_err(Into::into),
        RuntimePolicyCommands::Apply { language, file } => client
            .put_setting(
                SettingKind::Config,
                "foreign_languages",
                language,
                &params::load_json_file(file)?,
                None,
            )
            .await
            .map_err(Into::into),
        RuntimePolicyCommands::Delete { language } => client
            .delete_setting(SettingKind::Config, "foreign_languages", language)
            .await
            .map_err(Into::into),
    }
}

fn print_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(value);
    }
    print!("{}", output::value_table(value)?);
    Ok(())
}
