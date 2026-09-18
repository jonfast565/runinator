use super::*;

use runinator_ctl_core::cli::NotificationPolicyCommands;

pub(super) async fn notifications(
    client: &Client,
    command: &NotificationCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        NotificationCommands::List { unread, limit } => {
            client.fetch_notifications(*unread, *limit).await?
        }
        NotificationCommands::Read { id } => client.mark_notification_read(*id).await?,
        NotificationCommands::ReadAll => client.mark_all_notifications_read().await?,
        NotificationCommands::Delete { id } => client.delete_notification(*id).await?,
        NotificationCommands::Action { id, action, input } => {
            let input = input.as_deref().map(params::load_json_file).transpose()?;
            client.apply_notification_action(*id, action, input).await?
        }
        NotificationCommands::Policies { command } => match command {
            NotificationPolicyCommands::List { workflow } => {
                client.fetch_notification_policies(*workflow).await?
            }
            NotificationPolicyCommands::Apply { file, id } => {
                let policy = params::load_json_file(file)?;
                client.apply_notification_policy(*id, &policy).await?
            }
            NotificationPolicyCommands::Delete { id } => {
                client.delete_notification_policy(*id).await?
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
