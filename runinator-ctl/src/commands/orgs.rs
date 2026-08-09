use super::*;

pub(super) async fn orgs(client: &Client, command: &OrgCommands, json_output: bool) -> Result<()> {
    match command {
        OrgCommands::List => {
            let value = client.list_my_orgs().await?;
            output::json(&value)
        }
        OrgCommands::Create { name } => {
            let value = client.create_org(name).await?;
            if !json_output {
                println!("created organization '{name}'");
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
