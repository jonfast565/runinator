use super::*;

use runinator_models::workflow_vm::WorkflowEffectStatus;

pub(super) async fn gates(
    client: &Client,
    command: &GateCommands,
    json_output: bool,
) -> Result<()> {
    let value = match command {
        GateCommands::List { run_id, status } => {
            client.fetch_gates(*run_id, status.as_deref()).await?
        }
        GateCommands::Open { id, reason } | GateCommands::Close { id, reason } => {
            let open = matches!(command, GateCommands::Open { .. });
            let result = client
                .settle_workflow_effect(
                    *id,
                    WorkflowEffectStatus::Succeeded,
                    Some(json!({ "open": open, "reason": reason })),
                    reason.clone(),
                )
                .await?;
            serde_json::to_value(result)?.into()
        }
        GateCommands::Delete { id } => client.delete_gate(*id).await?,
    };
    if json_output {
        return output::json(&value);
    }
    print!("{}", output::value_table(&value)?);
    Ok(())
}
