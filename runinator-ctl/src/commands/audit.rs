use super::*;

pub(super) async fn audit(
    client: &Client,
    command: &AuditCommands,
    json_output: bool,
) -> Result<()> {
    let AuditCommands::List {
        actor,
        action,
        limit,
    } = command;
    let value = client
        .fetch_audit_log(*actor, action.as_deref(), *limit)
        .await?;
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
