use super::*;

use runinator_models::pipelines::{Pipeline, PipelineRun};

use runinator_ctl_core::cli::{CliTimelineFormat, PipelineCommands};

/// how often a followed pipeline run is re-read.
const FOLLOW_INTERVAL: Duration = Duration::from_secs(2);

pub(super) async fn pipelines(
    client: &Client,
    command: &PipelineCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        PipelineCommands::List => {
            let pipelines = client.fetch_pipelines().await?;
            if json_output {
                return output::json(&pipelines);
            }
            print_pipelines(&pipelines);
            Ok(())
        }
        PipelineCommands::Show { pipeline } => {
            let pipeline = resolve_pipeline(client, pipeline).await?;
            if json_output {
                return output::json(&pipeline);
            }
            print_pipeline(&pipeline);
            Ok(())
        }
        PipelineCommands::Run {
            pipeline,
            params,
            json_file,
            revision,
            from,
            follow,
        } => {
            let pipeline = resolve_pipeline(client, pipeline).await?;
            let pipeline_id = pipeline_id(&pipeline)?;
            let parameters = params::load_object(json_file.as_deref(), params)?;
            let run = client
                .create_pipeline_run_with_context(
                    pipeline_id,
                    parameters,
                    *revision,
                    from.as_deref(),
                )
                .await?;
            let run = match follow {
                true => follow_run(client, run.id).await?,
                false => run,
            };
            if json_output {
                return output::json(&run);
            }
            println!("pipeline run {} [{}]", run.id, run.status.as_str());
            Ok(())
        }
        PipelineCommands::Revisions { pipeline, limit } => {
            let pipeline = resolve_pipeline(client, pipeline).await?;
            let revisions = client
                .fetch_pipeline_revisions(pipeline_id(&pipeline)?, Some(*limit))
                .await?;
            if json_output {
                return output::json(&revisions);
            }
            let rows = revisions
                .iter()
                .map(|revision| {
                    vec![
                        revision.revision.to_string(),
                        revision.digest.clone(),
                        revision.source.to_string(),
                        revision.name.clone(),
                        output::time(revision.created_at),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(&["REV", "DIGEST", "SOURCE", "NAME", "CREATED"], &rows)
            );
            Ok(())
        }
        PipelineCommands::Revision { pipeline, revision } => {
            let pipeline = resolve_pipeline(client, pipeline).await?;
            let found = client
                .fetch_pipeline_revision(pipeline_id(&pipeline)?, *revision)
                .await?;
            if json_output {
                return output::json(&found);
            }
            println!("revision: {}", found.revision);
            println!("digest: {}", found.digest);
            println!("name: {}", found.name);
            println!("source: {}", found.source);
            println!("created_at: {}", output::time(found.created_at));
            println!("graph: {}", serde_json::to_string_pretty(&found.graph)?);
            Ok(())
        }
        PipelineCommands::Runs {
            pipeline,
            status,
            open,
        } => {
            let pipeline_id = match pipeline {
                Some(pipeline) => Some(pipeline_id(&resolve_pipeline(client, pipeline).await?)?),
                None => None,
            };
            let mut runs = client.fetch_pipeline_runs(pipeline_id).await?;
            if let Some(status) = status {
                let status = parse_workflow_status(status)?;
                runs.retain(|run| run.status == status);
            }
            if *open {
                runs.retain(|run| !run.status.is_terminal());
            }
            runs.sort_by_key(|run| std::cmp::Reverse(run.created_at));
            if json_output {
                return output::json(&runs);
            }
            print_runs(&runs);
            Ok(())
        }
        PipelineCommands::RunShow { run_id } => {
            let detail = client.fetch_pipeline_run(*run_id).await?;
            if json_output {
                return output::json(&detail);
            }
            println!(
                "pipeline run {} [{}]",
                detail.run.id,
                detail.run.status.as_str()
            );
            if let Some(message) = &detail.run.message {
                println!("{message}");
            }
            let rows = detail
                .members
                .iter()
                .map(|member| {
                    vec![
                        member.id.to_string(),
                        member.workflow_id.to_string(),
                        member.status.as_str().to_string(),
                        output::time(Some(member.created_at)),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(&["MEMBER RUN", "WORKFLOW", "STATUS", "CREATED"], &rows)
            );
            Ok(())
        }
        PipelineCommands::RunTimeline { run_id, format } => {
            let detail = client.fetch_pipeline_run(*run_id).await?;
            if json_output || matches!(format, CliTimelineFormat::Json) {
                return output::json(&detail);
            }
            match format {
                CliTimelineFormat::Table => print!("{}", timeline::pipeline_table(&detail)),
                CliTimelineFormat::Graph => print!("{}", timeline::pipeline_graph(&detail)),
                CliTimelineFormat::Json => unreachable!("json handled above"),
            }
            Ok(())
        }
        PipelineCommands::Cancel { run_id } => {
            let response = client.cancel_pipeline_run(*run_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
        PipelineCommands::DeleteRun { run_id } => {
            let response = client.delete_pipeline_run(*run_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
        PipelineCommands::Pause { run_id } => {
            let response = client.pause_pipeline_run(*run_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
        PipelineCommands::Resume { run_id } => {
            let response = client.resume_pipeline_run(*run_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
        PipelineCommands::Resolve {
            run_id,
            decision,
            by,
            message,
        } => {
            let run = client
                .resolve_pipeline_run(
                    *run_id,
                    decision.as_str(),
                    by.as_deref(),
                    message.as_deref(),
                )
                .await?;
            if json_output {
                return output::json(&run);
            }
            println!(
                "resolved pipeline run {} as {} [{}]",
                run.id,
                decision.as_str(),
                run.status.as_str()
            );
            Ok(())
        }
        PipelineCommands::Retry {
            run_id,
            member,
            params,
            json_file,
        } => {
            let parameters = params::load_object(json_file.as_deref(), params)?;
            let attempt = client
                .retry_pipeline_member(*run_id, member, parameters)
                .await?;
            if json_output {
                return output::json(&attempt);
            }
            println!(
                "retried {} as attempt {} [{}]",
                attempt.member_key,
                attempt.attempt,
                attempt.status.as_str()
            );
            Ok(())
        }
        PipelineCommands::Delete { pipeline } => {
            let pipeline = resolve_pipeline(client, pipeline).await?;
            let pipeline_id = pipeline_id(&pipeline)?;
            client.delete_pipeline(pipeline_id).await?;
            if json_output {
                return output::json(&json!({ "deleted": pipeline_id }));
            }
            println!("deleted pipeline {}", pipeline.name);
            Ok(())
        }
    }
}

/// Resolve a pipeline by UUID or its canonical `namespace.key` path. Display-name and bare-key
/// aliases are intentionally not accepted: names are presentation, not pipeline identity.
pub(super) async fn resolve_pipeline(client: &Client, reference: &str) -> Result<Pipeline> {
    if let Ok(id) = reference.parse::<Uuid>() {
        return Ok(client.fetch_pipeline(id).await?);
    }
    client
        .fetch_pipelines()
        .await?
        .into_iter()
        .find(|pipeline| {
            pipeline.namespace.is_some()
                && pipeline.key.is_some()
                && pipeline.artifact_path().qualified() == reference
        })
        .ok_or_else(|| err(format!("pipeline '{reference}' not found")))
}

fn pipeline_id(pipeline: &Pipeline) -> Result<Uuid> {
    pipeline
        .id
        .ok_or_else(|| err("pipeline has no persisted id"))
}

/// re-read the run until it settles.
async fn follow_run(client: &Client, run_id: Uuid) -> Result<PipelineRun> {
    loop {
        let detail = client.fetch_pipeline_run(run_id).await?;
        if detail.run.status.is_terminal() {
            return Ok(detail.run);
        }
        time::sleep(FOLLOW_INTERVAL).await;
    }
}

fn print_pipelines(pipelines: &[Pipeline]) {
    let rows = pipelines
        .iter()
        .map(|pipeline| {
            vec![
                pipeline
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".into()),
                pipeline.artifact_path().qualified(),
                pipeline.name.clone(),
                pipeline.graph.members.len().to_string(),
                pipeline.description.clone().unwrap_or_default(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(&["ID", "PATH", "NAME", "MEMBERS", "DESCRIPTION"], &rows)
    );
}

fn print_pipeline(pipeline: &Pipeline) {
    println!(
        "{} — {} ({})",
        pipeline.name,
        pipeline.artifact_path().qualified(),
        pipeline
            .id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unsaved".into())
    );
    if let Some(description) = &pipeline.description {
        println!("{description}");
    }
    let rows = pipeline
        .graph
        .members
        .iter()
        .map(|member| {
            vec![
                member.key.clone(),
                member.workflow_id.to_string(),
                member.failure_mode.as_str().to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(&["MEMBER", "WORKFLOW", "ON FAILURE"], &rows)
    );
}

fn print_runs(runs: &[PipelineRun]) {
    let rows = runs
        .iter()
        .map(|run| {
            vec![
                run.id.to_string(),
                run.pipeline_id.to_string(),
                run.status.as_str().to_string(),
                output::time(Some(run.created_at)),
                run.message.clone().unwrap_or_default(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(&["ID", "PIPELINE", "STATUS", "CREATED", "MESSAGE"], &rows)
    );
}
