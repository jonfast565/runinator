use super::*;

use runinator_ctl_core::cli::{AccessGrantCommands, AccessOwnerCommands};

pub(super) async fn access(
    client: &Client,
    command: &AccessCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        AccessCommands::Grants { command } => match command {
            AccessGrantCommands::List {
                resource_type,
                resource_id,
            } => {
                client
                    .fetch_resource_grants(resource_type, *resource_id)
                    .await?
            }
            AccessGrantCommands::Grant {
                resource_type,
                resource_id,
                principal_type,
                principal_id,
                permission,
            } => {
                client
                    .create_resource_grant(
                        resource_type,
                        *resource_id,
                        principal_type,
                        *principal_id,
                        permission,
                    )
                    .await?
            }
            AccessGrantCommands::Revoke {
                resource_type,
                resource_id,
                grant_id,
            } => {
                client
                    .revoke_resource_grant(resource_type, *resource_id, *grant_id)
                    .await?
            }
        },
        AccessCommands::Owner { command } => match command {
            AccessOwnerCommands::Show {
                resource_type,
                resource_id,
            } => {
                client
                    .fetch_resource_owner(resource_type, *resource_id)
                    .await?
            }
            AccessOwnerCommands::Transfer {
                resource_type,
                resource_id,
                scope_kind,
                scope_id,
            } => {
                client
                    .transfer_resource_owner(resource_type, *resource_id, scope_kind, *scope_id)
                    .await?
            }
        },
    };
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
