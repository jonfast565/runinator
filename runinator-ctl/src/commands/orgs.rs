use super::*;

pub(super) async fn orgs(
    client: &Client,
    command: &OrgCommands,
    api_base_url: &str,
    json_output: bool,
) -> Result<()> {
    match command {
        OrgCommands::List => {
            let value = client.list_my_orgs().await?;
            output::json(&value)
        }
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
        OrgCommands::Nodes { org } => {
            let value = client.fetch_org_nodes(*org).await?;
            output::json(&value)
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
            output::json(&value)
        }
    }
}
