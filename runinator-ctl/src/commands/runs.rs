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
            let run = client
                .fetch_workflow_runs(None, None)
                .await?
                .into_iter()
                .find(|run| run.id == *id)
                .ok_or_else(|| format!("workflow run {id} not found"))?;
            let continuations = client.fetch_workflow_continuations(*id).await?;
            let effects = client.fetch_workflow_effects(*id).await?;
            let journal = client.fetch_workflow_journal(*id).await?;
            if json_output {
                return output::json(&json!({
                    "run": run,
                    "continuations": continuations,
                    "effects": effects,
                    "journal": journal,
                }));
            }
            print_run_summary(&run);
            println!("continuations\t{}", continuations.len());
            println!("effects\t{}", effects.len());
            println!("journal entries\t{}", journal.len());
        }
        RunCommands::Watch {
            id,
            interval_seconds,
        } => loop {
            let run = client
                .fetch_workflow_runs(None, None)
                .await?
                .into_iter()
                .find(|run| run.id == *id)
                .ok_or_else(|| format!("workflow run {id} not found"))?;
            let continuations = client.fetch_workflow_continuations(*id).await?;
            let effects = client.fetch_workflow_effects(*id).await?;
            if json_output {
                output::json(
                    &json!({ "run": run, "continuations": continuations, "effects": effects }),
                )?;
            } else {
                print_run_summary(&run);
                println!(
                    "continuations\t{}\teffects\t{}",
                    continuations.len(),
                    effects.len()
                );
            }
            if run.status.is_terminal() {
                break;
            }
            time::sleep(Duration::from_secs(*interval_seconds)).await;
            if !json_output {
                println!();
            }
        },
        RunCommands::Logs { effect_id } => {
            let chunks = client.fetch_workflow_effect_output(*effect_id).await?;
            if json_output {
                return output::json(&chunks);
            }
            for event in chunks {
                let runinator_models::workflow_vm::WorkflowEffectOutput::Chunk { content, .. } =
                    event.output
                else {
                    continue;
                };
                print!("{content}");
                if !content.ends_with('\n') {
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
            let effects = client.fetch_workflow_effects(*id).await?;
            let mut artifacts = Vec::new();
            for effect in effects {
                artifacts.extend(
                    client
                        .fetch_workflow_effect_output(effect.id)
                        .await?
                        .into_iter()
                        .filter(|event| {
                            matches!(
                                event.output,
                                runinator_models::workflow_vm::WorkflowEffectOutput::Artifact { .. }
                            )
                        }),
                );
            }
            if json_output {
                return output::json(&artifacts);
            }
            if artifacts.is_empty() {
                println!("no artifacts for run {id}");
                return Ok(());
            }
            for event in artifacts {
                if let runinator_models::workflow_vm::WorkflowEffectOutput::Artifact { artifact } =
                    event.output
                {
                    println!("{}\t{}", event.effect_id, serde_json::to_string(&artifact)?);
                }
            }
        }
    }
    Ok(())
}
