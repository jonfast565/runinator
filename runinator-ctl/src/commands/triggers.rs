use super::*;

pub(super) async fn triggers(
    client: &Client,
    command: &TriggerCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        TriggerCommands::List { workflow } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = workflow
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let triggers = client.fetch_workflow_triggers(workflow_id).await?;
            if json_output {
                return output::json(&triggers);
            }
            print_triggers(&triggers);
        }
        TriggerCommands::Due => {
            let triggers = client.fetch_due_workflow_triggers().await?;
            if json_output {
                return output::json(&triggers);
            }
            print_triggers(&triggers);
        }
        TriggerCommands::Backfill {
            trigger_id,
            from,
            to,
            limit,
            dry_run,
        } => {
            let request = BackfillRequest {
                from: *from,
                to: to.unwrap_or_else(Utc::now),
                limit: *limit,
                dry_run: *dry_run,
            };
            let response = client
                .backfill_workflow_trigger(*trigger_id, &request)
                .await?;
            if json_output {
                return output::json(&response);
            }
            let verb = match response.dry_run {
                true => "would fire",
                false => "fired",
            };
            println!(
                "{verb} {} slot(s); {} already fired{}",
                response.fired,
                response.already_fired,
                match response.truncated {
                    true => " (range truncated by limit)",
                    false => "",
                }
            );
            for slot in &response.slots {
                println!("  {slot}");
            }
        }
        TriggerCommands::Run {
            trigger_id,
            params: cli_params,
            json_file,
            debug,
        } => {
            let payload = params::load_object(json_file.as_deref(), cli_params)?;
            let run = client
                .create_workflow_trigger_run(*trigger_id, payload, *debug)
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
    }
    Ok(())
}
