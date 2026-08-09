use super::*;

pub(super) async fn approvals(
    client: &Client,
    command: &ApprovalCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ApprovalCommands::List {
            workflow_run_id,
            open,
        } => {
            let mut approvals = client.fetch_approvals(*workflow_run_id).await?;
            if *open {
                approvals.retain(|approval| value_str(approval, "status") == Some("open"));
            }
            if json_output {
                return output::json(&approvals);
            }
            print_approvals(&approvals);
        }
        ApprovalCommands::Approve {
            id,
            by,
            message,
            json_file,
        } => {
            let output_json = optional_json(json_file)?;
            let approval = client
                .approve_request(*id, by.clone(), message.clone(), output_json)
                .await?;
            if json_output {
                return output::json(&approval);
            }
            println!("approved request {id}");
        }
        ApprovalCommands::Reject {
            id,
            by,
            message,
            json_file,
        } => {
            let output_json = optional_json(json_file)?;
            let approval = client
                .reject_request(*id, by.clone(), message.clone(), output_json)
                .await?;
            if json_output {
                return output::json(&approval);
            }
            println!("rejected request {id}");
        }
    }
    Ok(())
}
