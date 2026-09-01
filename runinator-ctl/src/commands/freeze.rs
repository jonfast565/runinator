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
    if windows.is_empty() {
        println!("No freeze windows.");
        return;
    }
    for window in windows {
        let scope = match (window.workflow_id, window.org_id) {
            (Some(workflow_id), _) => format!("workflow {workflow_id}"),
            (None, Some(org_id)) => format!("org {org_id}"),
            (None, None) => "platform".to_string(),
        };
        let state = match window.enabled {
            true => "enabled",
            false => "disabled",
        };
        println!(
            "{}  {}  {} -> {}  [{scope}, {state}]",
            window.id, window.name, window.starts_at, window.ends_at
        );
        if let Some(reason) = &window.reason {
            println!("    {reason}");
        }
    }
}
