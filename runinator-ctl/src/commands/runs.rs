use super::*;

use runinator_comm::DebugVerb;
use runinator_ctl_core::cli::{BreakpointCommands, CliTimelineFormat, TerminalCommands};
use runinator_models::{runs::ProviderTerminalControl, workflow_vm::WorkflowEffectStatus};

const WATCH_INTERVAL_MINIMUM: u64 = 1;

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
            let run = client.fetch_workflow_run(*id).await?;
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
            print!(
                "{}",
                output::table(
                    &["continuations", "effects", "journal_entries"],
                    &[vec![
                        continuations.len().to_string(),
                        effects.len().to_string(),
                        journal.len().to_string(),
                    ]],
                )
            );
        }
        RunCommands::Timeline { id, format } => {
            print_workflow_timeline(client, *id, *format, json_output).await?;
        }
        RunCommands::Watch {
            id,
            interval_seconds,
            format,
        } => {
            let machine_output = json_output || matches!(format, CliTimelineFormat::Json);
            let mut display = output::LiveDisplay::new(machine_output);
            loop {
                display.begin_frame();
                let run = print_workflow_timeline(client, *id, *format, json_output).await?;
                display.flush()?;
                if run.status.is_terminal() {
                    break;
                }
                time::sleep(Duration::from_secs(
                    (*interval_seconds).max(WATCH_INTERVAL_MINIMUM),
                ))
                .await;
            }
        }
        RunCommands::Usage { id } => {
            let value = client.fetch_run_ai_usage(*id).await?;
            if json_output {
                return output::json(&value);
            }
            print!("{}", output::value_table(&value)?);
        }
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
        RunCommands::Step { id, cursor } => print_task_response(
            client
                .debug_workflow_run(*id, &DebugVerb::Step { cursor: *cursor })
                .await?,
            "stepped workflow run",
            json_output,
        )?,
        RunCommands::Continue { id, cursor } => print_task_response(
            client
                .debug_workflow_run(*id, &DebugVerb::Continue { cursor: *cursor })
                .await?,
            "continued workflow run",
            json_output,
        )?,
        RunCommands::Breakpoints { command } => {
            let (id, breakpoints) = match command {
                BreakpointCommands::Set { id, breakpoints } => (*id, breakpoints.clone()),
                BreakpointCommands::Clear { id } => (*id, Vec::new()),
            };
            print_task_response(
                client
                    .debug_workflow_run(id, &DebugVerb::SetBreakpoints { breakpoints })
                    .await?,
                "updated workflow breakpoints",
                json_output,
            )?;
        }
        RunCommands::ToNode {
            id,
            cursor,
            node_id,
        } => print_task_response(
            client
                .debug_workflow_run(
                    *id,
                    &DebugVerb::RunTo {
                        cursor: *cursor,
                        node_id: node_id.clone(),
                    },
                )
                .await?,
            "continued workflow run to node",
            json_output,
        )?,
        RunCommands::PauseOnFailure { id, enabled } => print_task_response(
            client
                .debug_workflow_run(*id, &DebugVerb::SetPauseOnFailure { enabled: *enabled })
                .await?,
            "updated pause-on-failure",
            json_output,
        )?,
        RunCommands::Signal {
            id,
            name,
            json_file,
        } => {
            let payload = match json_file {
                Some(path) => params::load_json_file(path)?,
                None => Value::Object(Map::new()),
            };
            print_task_response(
                client.deliver_workflow_signal(*id, name, payload).await?,
                "delivered workflow signal",
                json_output,
            )?;
        }
        RunCommands::Interrupt {
            id,
            source,
            json_file,
            continuation,
        } => {
            let payload = match json_file {
                Some(path) => params::load_json_file(path)?,
                None => Value::Null,
            };
            print_task_response(
                client
                    .request_workflow_interrupt(*id, source, payload, *continuation)
                    .await?,
                "recorded workflow interrupt",
                json_output,
            )?;
        }
        RunCommands::ResolveInput {
            effect,
            json_file,
            message,
        } => print_task_response(
            client
                .settle_workflow_effect(
                    *effect,
                    WorkflowEffectStatus::Succeeded,
                    Some(params::load_json_file(json_file)?),
                    message.clone(),
                )
                .await?,
            "resolved workflow input",
            json_output,
        )?,
        RunCommands::Terminal { command } => {
            let (effect, control) = match command {
                TerminalCommands::Input { effect, data } => (
                    *effect,
                    ProviderTerminalControl::Input { data: data.clone() },
                ),
                TerminalCommands::Resize { effect, cols, rows } => (
                    *effect,
                    ProviderTerminalControl::Resize {
                        cols: *cols,
                        rows: *rows,
                    },
                ),
                TerminalCommands::Close { effect } => (*effect, ProviderTerminalControl::Eof),
            };
            print_task_response(
                client
                    .control_workflow_effect_terminal(effect, control)
                    .await?,
                "sent workflow terminal control",
                json_output,
            )?;
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
        RunCommands::ReplayPlan { id, from_step_id } => {
            let plan = client
                .workflow_replay_plan(*id, from_step_id.as_deref())
                .await?;
            if json_output {
                return output::json(&plan);
            }
            print!("{}", output::value_table(&plan)?);
        }
        RunCommands::Replay {
            id,
            from_step_id,
            acknowledge_review,
        } => {
            let plan = client
                .workflow_replay_plan(*id, from_step_id.as_deref())
                .await?;
            if !json_output {
                output::json(&plan)?;
            }
            let run = client
                .replay_workflow_run_reviewed(
                    *id,
                    &runinator_models::replay::ReplayOptions {
                        from_step_id: from_step_id.clone(),
                        plan_fingerprint: Some(plan.plan_fingerprint),
                        acknowledge_review: *acknowledge_review,
                    },
                )
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
            artifacts::print_artifacts(&artifacts);
        }
    }
    Ok(())
}

async fn print_workflow_timeline(
    client: &Client,
    id: Uuid,
    format: CliTimelineFormat,
    json_output: bool,
) -> Result<WorkflowRun> {
    let run = client.fetch_workflow_run(id).await?;
    let journal = client.fetch_workflow_journal(id).await?;
    let effects = client.fetch_workflow_effects(id).await?;
    if json_output || matches!(format, CliTimelineFormat::Json) {
        let continuations = client.fetch_workflow_continuations(id).await?;
        let transitions = client.fetch_workflow_run_transitions(id).await?;
        output::json(&json!({
            "run": run.clone(),
            "journal": journal,
            "effects": effects,
            "continuations": continuations,
            "transitions": transitions,
        }))?;
    } else {
        match format {
            CliTimelineFormat::Table => {
                print!("{}", timeline::workflow_table(&run, &journal, &effects))
            }
            CliTimelineFormat::Graph => {
                print!("{}", timeline::workflow_graph(&run, &journal, &effects))
            }
            CliTimelineFormat::Json => unreachable!("json handled above"),
        }
    }
    Ok(run)
}
