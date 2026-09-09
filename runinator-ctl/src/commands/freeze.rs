use super::*;

pub(super) async fn freeze(
    client: &Client,
    command: &FreezeCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        FreezeCommands::List { active } => {
            let windows = client.fetch_freeze_windows(*active).await?;
            if json_output {
                return output::json(&windows);
            }
            print_freeze_windows(&windows);
        }
        FreezeCommands::Create {
            name,
            from,
            to,
            workflow_id,
            org_id,
            reason,
        } => {
            let window = client
                .create_freeze_window(&NewFreezeWindow {
                    org_id: *org_id,
                    workflow_id: *workflow_id,
                    name: name.clone(),
                    reason: reason.clone(),
                    starts_at: *from,
                    ends_at: *to,
                    schedule: None,
                    enabled: true,
                })
                .await?;
            if json_output {
                return output::json(&window);
            }
            print_freeze_windows(std::slice::from_ref(&window));
        }
        FreezeCommands::Delete { window_id } => {
            let response = client.delete_freeze_window(*window_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
        }
    }
    Ok(())
}

fn print_freeze_windows(windows: &[FreezeWindow]) {
    let rows = windows
        .iter()
        .map(|window| {
            let scope = match (window.workflow_id, window.org_id) {
                (Some(workflow_id), _) => format!("workflow {workflow_id}"),
                (None, Some(org_id)) => format!("org {org_id}"),
                (None, None) => "platform".to_string(),
            };
            vec![
                window.id.to_string(),
                window.name.clone(),
                window.starts_at.to_rfc3339(),
                window.ends_at.to_rfc3339(),
                scope,
                if window.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
                .into(),
                window.reason.clone().unwrap_or_else(|| "-".into()),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &["id", "name", "from", "to", "scope", "state", "reason"],
            &rows,
        )
    );
}
