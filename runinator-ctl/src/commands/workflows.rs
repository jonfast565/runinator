use super::*;

pub(super) async fn workflows(
    client: &Client,
    command: &WorkflowCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        WorkflowCommands::List => {
            let workflows = client.fetch_workflows().await?;
            if json_output {
                return output::json(&workflows);
            }
            print_workflows(&workflows);
        }
        WorkflowCommands::Show { workflow } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            if json_output {
                return output::json(&workflow);
            }
            print_workflow(&workflow)?;
        }
        WorkflowCommands::Validate { file } => {
            let workflow = read_workflow_definition(file)?;
            let workflow = client.validate_workflow(&workflow).await?;
            if json_output {
                return output::json(&workflow);
            }
            println!("workflow {} v{} validates", workflow.name, workflow.version);
        }
        WorkflowCommands::Apply {
            file,
            contract_override_reason,
        } => {
            let resolved = resolve_workflow_apply_path(file.as_deref())?;
            let summary = apply_workflow_source(
                client,
                &resolved,
                json_output,
                contract_override_reason.as_deref(),
            )
            .await?;
            if !json_output {
                print_apply_summary(&summary);
            }
        }
        WorkflowCommands::Test {
            file,
            tests,
            filter,
        } => {
            return workflows_test(file, tests, filter.as_deref(), json_output);
        }
        WorkflowCommands::Dev {
            file,
            run,
            params: cli_params,
            json_file,
            debug,
            name,
            watch_interval_ms,
            debounce_ms,
        } => {
            if json_output {
                return Err(err("workflows dev does not support --json output"));
            }
            let resolved = resolve_workflow_apply_path(file.as_deref())?;
            workflow_dev(
                client,
                WorkflowDevRequest {
                    file: &resolved,
                    run_workflow: run.as_deref(),
                    cli_params,
                    json_file: json_file.as_deref(),
                    debug: *debug,
                    name: name.as_deref(),
                    watch_interval: Duration::from_millis(*watch_interval_ms),
                    debounce: Duration::from_millis(*debounce_ms),
                },
            )
            .await?;
        }
        WorkflowCommands::Export {
            workflow_id,
            output: path,
        } => {
            let bundle = client.export_workflow_bundle(*workflow_id).await?;
            if let Some(path) = path {
                write_json_file(path, &bundle)?;
                if !json_output {
                    println!("wrote {}", path.display());
                }
            }
            if json_output || path.is_none() {
                output::json(&bundle)?;
            }
        }
        WorkflowCommands::Revisions { workflow, limit } => {
            let existing = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = existing
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let revisions = client
                .fetch_workflow_revisions(workflow_id, Some(*limit))
                .await?;
            if json_output {
                return output::json(&revisions);
            }
            print_workflow_revisions(&revisions);
        }
        WorkflowCommands::Revision { workflow, revision } => {
            let existing = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = existing
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let found = client
                .fetch_workflow_revision(workflow_id, *revision)
                .await?;
            if json_output {
                return output::json(&found);
            }
            println!("revision: {}", found.revision);
            println!("name: {}", found.name);
            println!("version: {}", found.version);
            println!("source: {}", found.source);
            println!("author: {}", revision_author_label(&found));
            if let Some(note) = &found.note {
                println!("note: {note}");
            }
            println!("created_at: {}", output::time(found.created_at));
            println!(
                "definition: {}",
                serde_json::to_string_pretty(&found.definition)?
            );
        }
        WorkflowCommands::Rollback { workflow, revision } => {
            let existing = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = existing
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let restored = client
                .restore_workflow_revision(workflow_id, *revision)
                .await?;
            if json_output {
                return output::json(&restored);
            }
            println!(
                "restored {} to revision {} (saved as a new revision)",
                restored.name, revision
            );
        }
        WorkflowCommands::Duplicate { workflow, bump } => {
            let existing = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = existing
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let copy = client
                .duplicate_workflow(workflow_id, (*bump).into())
                .await?;
            if json_output {
                return output::json(&copy);
            }
            println!(
                "duplicated {} -> id {} v{}",
                existing.name,
                copy.id.unwrap_or_default(),
                copy.version
            );
        }
        WorkflowCommands::Run {
            workflow,
            params: cli_params,
            json_file,
            debug,
            name,
        } => {
            let workflow = fetch_workflow_ref(client, workflow).await?;
            let workflow_id = workflow
                .id
                .ok_or_else(|| err("workflow has no persisted id"))?;
            let payload = params::load_object(json_file.as_deref(), cli_params)?;
            let run = client
                .create_workflow_run_with_options(workflow_id, payload, *debug, name.clone())
                .await?;
            if json_output {
                return output::json(&run);
            }
            print_run_summary(&run);
        }
    }
    Ok(())
}

fn resolve_workflow_apply_path(file: Option<&Path>) -> Result<PathBuf> {
    match file {
        Some(path) => Ok(path.to_path_buf()),
        None => {
            let fallback = runinator_platform::app_data::app_data_path("workflows")
                .map_err(|e| err(e.to_string()))?;
            if !fallback.exists() {
                return Err(err(format!(
                    "no file or folder given and no default workflows folder at {}",
                    fallback.display()
                )));
            }
            Ok(fallback)
        }
    }
}

async fn apply_workflow_source(
    client: &Client,
    file: &Path,
    json_output: bool,
    contract_override_reason: Option<&str>,
) -> Result<WorkflowApplySummary> {
    // a .rexrap/.rexrapm/directory is compiled client-side, zipped, and uploaded as one compiled pack;
    // json is handled below.
    if pack::is_pack_source(file) {
        // Discover local function packages before compiling. A workflow in this pack may call an
        // export it publishes, whose permanent UUIDs do not exist until the server accepts this
        // import; temporary catalog entries make that call type-check and bind deterministically.
        let function_sources = runinator_pack::functions::discover_function_sources(file)?;
        // both halves of the catalog: provider metadata types ordinary actions, and the published
        // function entries are what a `functions.<pkg>.<export>(...)` call binds against. fetched
        // together because a compile given only one silently loses the ability to resolve the other.
        let mut catalog = pack::PackCatalog {
            providers: client.fetch_providers().await.unwrap_or_default(),
            functions: client.fetch_function_catalog().await.unwrap_or_default(),
        };
        catalog.functions.extend(
            function_sources
                .iter()
                .flat_map(|source| source.provisional_catalog_entries()),
        );
        let bundle = pack::load_workflow_bundle_with_catalog(file, &catalog)?;
        // function packages the pack carries, published as part of the same apply. discovered from
        // the source tree rather than declared in the manifest: the manifest lists what to compile,
        // and a package directory is already self-identifying by its own manifest file.
        // any settings (`settings.rexraps`/`.json`) always ride in the same compiled pack zip.
        let settings = pack::load_pack_settings(file)?;
        // any pipelines (`.rexrapp` files) ride along too; the backend upserts them and materializes
        // their managed chained triggers after the workflows land.
        let pipelines = pack::load_pack_pipelines(file)?;
        // `workflows apply` is an explicit re-apply: update existing items in place.
        let result = if contract_override_reason.is_some() {
            let mut builder = runinator_pack_wire::pack::PackBuilder::new(&bundle)
                .settings(settings.as_ref())
                .pipelines(pipelines.as_ref())
                .functions(
                    function_sources
                        .iter()
                        .map(|source| source.publish_request())
                        .collect(),
                );
            for source in &function_sources {
                if client
                    .fetch_function_artifact(&source.archive.digest)
                    .await?
                    .is_none()
                {
                    builder = builder.function_artifact(
                        source.archive.digest.clone(),
                        source.archive.bytes.clone(),
                    );
                }
            }
            client
                .import_reviewed_pack_zip(builder.build()?, true, contract_override_reason)
                .await?
        } else if function_sources.is_empty() {
            client
                .import_pack(&bundle, settings.as_ref(), pipelines.as_ref(), true)
                .await?
        } else {
            let functions = function_sources
                .iter()
                .map(|source| source.publish_request())
                .collect();
            let artifacts = function_sources
                .iter()
                .map(|source| (source.archive.digest.clone(), source.archive.bytes.clone()))
                .collect();
            client
                .import_pack_with_functions(
                    &bundle,
                    settings.as_ref(),
                    pipelines.as_ref(),
                    functions,
                    artifacts,
                    true,
                )
                .await?
        };
        let summary = WorkflowApplySummary {
            message: format!(
                "imported {} workflows, {} triggers, {} settings, and {} pipelines",
                result.workflows.workflows.len(),
                result.workflows.triggers.len(),
                result.settings.settings.len(),
                result.pipelines.len()
            ),
        };
        if json_output {
            output::json(&result)?;
        }
        return Ok(summary);
    }

    let value = params::load_json_file(file)?;
    if value.get("workflows").is_some() {
        return Err(
            "raw workflow bundles are no longer accepted; apply a .rrx source or compiled pack instead"
                .into(),
        );
    }

    let workflow: WorkflowDefinition = serde_json::from_value(value.into())?;
    let workflow = client
        .publish_workflow(&workflow, contract_override_reason)
        .await?;
    if json_output {
        output::json(&workflow)?;
    }
    Ok(WorkflowApplySummary {
        message: format!(
            "saved workflow {} v{} id={}",
            workflow.name,
            workflow.version,
            workflow.id.unwrap_or_default()
        ),
    })
}

fn print_apply_summary(summary: &WorkflowApplySummary) {
    println!("{}", summary.message);
}

// dry-run a compiled pack against .rexrapt suites entirely client-side; no server or broker involved.
struct WorkflowDevRequest<'a> {
    file: &'a Path,
    run_workflow: Option<&'a str>,
    cli_params: &'a [String],
    json_file: Option<&'a Path>,
    debug: bool,
    name: Option<&'a str>,
    watch_interval: Duration,
    debounce: Duration,
}

async fn workflow_dev(client: &Client, request: WorkflowDevRequest<'_>) -> Result<()> {
    let WorkflowDevRequest {
        file,
        run_workflow,
        cli_params,
        json_file,
        debug,
        name,
        watch_interval,
        debounce,
    } = request;
    if watch_interval.is_zero() {
        return Err(err("--watch-interval-ms must be greater than 0"));
    }

    println!("watching {}", file.display());
    if let Some(path) = json_file {
        println!("watching run input {}", path.display());
    }
    println!("press Ctrl-C to stop");
    println!();

    let mut last_snapshot: Option<SourceSnapshot> = None;
    loop {
        let mut snapshot = source_snapshot(file, json_file);
        let changed = last_snapshot
            .as_ref()
            .map(|previous| previous != &snapshot)
            .unwrap_or(true);

        if changed {
            if last_snapshot.is_some() && !debounce.is_zero() {
                time::sleep(debounce).await;
                snapshot = source_snapshot(file, json_file);
            }

            let source_count = snapshot.files.len();
            println!(
                "[dev] applying {} source file{}",
                source_count,
                if source_count == 1 { "" } else { "s" }
            );
            match apply_workflow_source(client, file, false, None).await {
                Ok(summary) => {
                    print_apply_summary(&summary);
                    if let Some(workflow) = run_workflow
                        && let Err(err) =
                            dev_run_workflow(client, workflow, cli_params, json_file, debug, name)
                                .await
                    {
                        eprintln!("[dev] run failed:\n{err}");
                    }
                }
                Err(err) => {
                    eprintln!("[dev] apply failed:\n{err}");
                }
            }
            println!();
            last_snapshot = Some(snapshot);
        }

        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|signal_err| {
                    err(format!("failed to listen for Ctrl-C: {signal_err}"))
                })?;
                println!("stopped workflow dev watcher");
                break;
            }
            _ = time::sleep(watch_interval) => {}
        }
    }

    Ok(())
}

async fn dev_run_workflow(
    client: &Client,
    workflow_ref: &str,
    cli_params: &[String],
    json_file: Option<&Path>,
    debug: bool,
    name: Option<&str>,
) -> Result<()> {
    let workflow = fetch_workflow_ref(client, workflow_ref).await?;
    let workflow_id = workflow
        .id
        .ok_or_else(|| err("workflow has no persisted id"))?;
    let payload = params::load_object(json_file, cli_params)?;
    let run = client
        .create_workflow_run_with_options(
            workflow_id,
            payload,
            debug,
            name.map(ToString::to_string),
        )
        .await?;
    print_run_summary(&run);
    watch_run_until_terminal(client, run.id, Duration::from_secs(1)).await
}

async fn watch_run_until_terminal(client: &Client, run_id: Uuid, interval: Duration) -> Result<()> {
    loop {
        let run = client
            .fetch_workflow_runs(None, None)
            .await?
            .into_iter()
            .find(|run| run.id == run_id)
            .ok_or_else(|| format!("workflow run {run_id} not found"))?;
        let continuations = client.fetch_workflow_continuations(run_id).await?;
        let effects = client.fetch_workflow_effects(run_id).await?;
        print_run_summary(&run);
        println!(
            "continuations\t{}\teffects\t{}",
            continuations.len(),
            effects.len()
        );
        if run.status.is_terminal() {
            return Ok(());
        }
        time::sleep(interval).await;
        println!();
    }
}

fn source_snapshot(file: &Path, json_file: Option<&Path>) -> SourceSnapshot {
    let mut paths = match pack::pack_source_files(file) {
        Ok(paths) if !paths.is_empty() => paths,
        _ => vec![file.to_path_buf()],
    };
    if let Some(path) = json_file {
        paths.push(path.to_path_buf());
    }
    paths.sort();
    paths.dedup();

    let files = paths
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(&path).ok();
            SourceFileSnapshot {
                path,
                modified: metadata.as_ref().and_then(|meta| meta.modified().ok()),
                len: metadata.as_ref().map(|meta| meta.len()),
            }
        })
        .collect();
    SourceSnapshot { files }
}

mod workflow_tests;
pub use workflow_tests::workflows_test;
mod rexrap;
pub(super) use rexrap::rexrap;
