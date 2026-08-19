use super::*;

pub(super) async fn runs(client: &Client, command: &RunCommands, json_output: bool) -> Result<()> {
    match command {
        RunCommands::List {
            status,
            workflow_id,
            open,
        } => {
            let runs = fetch_runs(client, status.as_deref(), *workflow_id, *open).await?;
            if json_output {
                return output::json(&runs);
            }
            print_runs(&runs);
        }
        RunCommands::Show { id } => {
            let (run, nodes) = client.fetch_workflow_run(*id).await?;
            if json_output {
                return output::json(&json!({ "run": run, "nodes": nodes }));
            }
            print_run_detail(&run, &nodes);
        }
        RunCommands::Watch {
            id,
            interval_seconds,
        } => loop {
            let (run, nodes) = client.fetch_workflow_run(*id).await?;
            if json_output {
                output::json(&json!({ "run": run, "nodes": nodes }))?;
            } else {
                print_run_detail(&run, &nodes);
            }
            if run.status.is_terminal() {
                break;
            }
            time::sleep(Duration::from_secs(*interval_seconds)).await;
            if !json_output {
                println!();
            }
        },
        RunCommands::Logs {
            node_run_id,
            cursor,
            limit,
        } => {
            let chunks = client
                .fetch_workflow_node_run_chunks(*node_run_id, *cursor, *limit)
                .await?;
            if json_output {
                return output::json(&chunks);
            }
            for chunk in chunks {
                print!("{}", chunk.content);
                if !chunk.content.ends_with('\n') {
                    println!();
                }
            }
        }
        RunCommands::Pause { id } => print_task_response(
            client.pause_workflow_run(*id).await?,
            "paused workflow run",
            json_output,
        )?,
        RunCommands::Resume { id } => print_task_response(
            client.resume_workflow_run(*id).await?,
            "resumed workflow run",
            json_output,
        )?,
        RunCommands::Cancel { id } => print_task_response(
            client.cancel_workflow_run(*id).await?,
            "canceled workflow run",
            json_output,
        )?,
        RunCommands::Delete { id } => print_task_response(
            client.delete_workflow_run(*id).await?,
            "deleted workflow run",
            json_output,
        )?,
        RunCommands::Replay { id, from_step_id } => {
            let run = client
                .replay_workflow_run(*id, from_step_id.clone())
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
        RunCommands::Rename { id, name } => print_task_response(
            client.rename_workflow_run(*id, name.clone()).await?,
            "renamed workflow run",
            json_output,
        )?,
        RunCommands::Artifacts { id } => {
            let artifacts = client.fetch_workflow_run_artifacts(*id).await?;
            if json_output {
                return output::json(&artifacts);
            }
            if artifacts.is_empty() {
                println!("no artifacts for run {id}");
                return Ok(());
            }
            for artifact in artifacts {
                println!(
                    "{}\t{}\t{}\t{} bytes\t{}",
                    artifact.id,
                    artifact.name,
                    artifact.mime_type,
                    artifact.size_bytes,
                    artifact.uri
                );
            }
        }
    }
    Ok(())
}
