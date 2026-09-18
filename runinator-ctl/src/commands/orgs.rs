use super::*;
use runinator_ctl_core::cli::OrgMemberCommands;

pub(super) async fn orgs(
    client: &Client,
    command: &OrgCommands,
    api_base_url: &str,
    json_output: bool,
) -> Result<()> {
    match command {
        OrgCommands::List => {
            let value = client.list_my_orgs().await?;
            if json_output {
                return output::json(&value);
            }
            print!("{}", output::value_table(&value)?);
            Ok(())
        }
        OrgCommands::All => print_value(&client.list_all_orgs().await?, json_output),
        OrgCommands::Use { org } => {
            let context = client.switch_org(*org).await?;
            crate::auth::persist_active_scope(
                api_base_url,
                context.access_token.clone(),
                Some(*org),
            )?;
            if json_output {
                output::json(&serde_json::json!({
                    "active_scope": "organization",
                    "org_id": org,
                    "org": context.org,
                    "role": context.role,
                }))
            } else {
                println!("active organization: {}", context.org.name);
                Ok(())
            }
        }
        OrgCommands::Platform => {
            let context = client.switch_platform().await?;
            crate::auth::persist_active_scope(api_base_url, context.access_token, None)?;
            if json_output {
                output::json(&serde_json::json!({ "active_scope": "platform" }))
            } else {
                println!("active scope: platform");
                Ok(())
            }
        }
        OrgCommands::Create { name } => {
            let value = client.create_org(name).await?;
            if !json_output {
                println!("created organization '{name}'");
            }
            output::json(&value)
        }
        OrgCommands::Rename { org, name } => {
            let value = client.rename_org(*org, name).await?;
            if !json_output {
                println!("renamed organization '{org}' to '{name}'");
            }
            output::json(&value)
        }
        OrgCommands::Delete { org } => print_value(&client.delete_org(*org).await?, json_output),
        OrgCommands::Members { command } => {
            let value = match command {
                OrgMemberCommands::List { org } => client.fetch_org_members(*org).await?,
                OrgMemberCommands::Add { org, user, role } => {
                    client.add_org_member(*org, *user, role).await?
                }
                OrgMemberCommands::Update { org, user, role } => {
                    client.update_org_member(*org, *user, role).await?
                }
                OrgMemberCommands::Remove { org, user } => {
                    client.remove_org_member(*org, *user).await?
                }
            };
            print_value(&value, json_output)
        }
        OrgCommands::Nodes { org } => {
            let value = client.fetch_org_nodes(*org).await?;
            if json_output {
                return output::json(&value);
            }
            print!("{}", output::value_table(&value)?);
            Ok(())
        }
        OrgCommands::Scale {
            org,
            backend,
            kind,
            desired,
        } => {
            let request = ScaleOrgNodesRequest {
                backend: (*backend).into(),
                kind: (*kind).into(),
                desired: *desired,
            };
            let value = client.scale_org_nodes(*org, &request).await?;
            output::json(&value)
        }
        OrgCommands::Usage { org } => {
            let value = client.fetch_org_usage(*org).await?;
            if json_output {
                return output::json(&value);
            }
            print!("{}", output::value_table(&value)?);
            Ok(())
        }
        OrgCommands::Quota { org } => {
            print_value(&client.fetch_org_quota(*org).await?, json_output)
        }
    }
}

fn print_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(value);
    }
    print!("{}", output::value_table(value)?);
    Ok(())
}
