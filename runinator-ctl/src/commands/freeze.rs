use super::*;
use runinator_ctl_core::cli::CalendarCommands;

pub(super) async fn freeze(
    client: &Client,
    command: &FreezeCommands,
    api_base_url: &str,
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
        FreezeCommands::Update { window_id, file } => {
            let window: NewFreezeWindow = serde_json::from_slice(&fs::read(file)?)?;
            let saved = client.update_freeze_window(*window_id, &window).await?;
            if json_output {
                return output::json(&saved);
            }
            print_freeze_windows(std::slice::from_ref(&saved));
        }
        FreezeCommands::Calendar { command } => match command {
            CalendarCommands::Subscribe { scope, org_id } => {
                let subscription = client
                    .create_calendar_subscription(scope.as_str(), *org_id)
                    .await?;
                output::json(&subscription)?;
            }
            CalendarCommands::Unsubscribe { subscription_id } => {
                client
                    .delete_calendar_subscription(*subscription_id)
                    .await?;
                if json_output {
                    output::json(&json!({ "deleted": true, "id": subscription_id }))?;
                } else {
                    println!("revoked calendar subscription {subscription_id}");
                }
            }
            CalendarCommands::Url { token } => {
                let url = format!(
                    "{}/calendar/{token}/runinator.ics",
                    api_base_url.trim_end_matches('/')
                );
                if json_output {
                    output::json(&json!({ "url": url }))?;
                } else {
                    println!("{url}");
                }
            }
            CalendarCommands::Download {
                scope,
                org_id,
                output: path,
            } => {
                let bytes = client
                    .download_schedule_calendar(scope.as_str(), *org_id)
                    .await?;
                fs::write(path, bytes)?;
                if json_output {
                    output::json(&json!({ "output": path }))?;
                } else {
                    println!("wrote {}", path.display());
                }
            }
        },
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
