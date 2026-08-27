use super::*;

use runinator_models::orchestration::{OrchestrationBinding, OrchestrationEventReduction};

use crate::cli::{OrchestrationAdapterCommands, OrchestrationCommands};

const WATCH_INTERVAL_MINIMUM: u64 = 1;

pub(super) async fn orchestrations(
    client: &Client,
    command: &OrchestrationCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        OrchestrationCommands::List {
            status,
            pipeline_id,
            scope,
            correlation,
        } => {
            let bindings = client
                .fetch_orchestrations(
                    status.as_deref(),
                    *pipeline_id,
                    scope.as_deref(),
                    correlation.as_deref(),
                )
                .await?;
            if json_output {
                return output::json(&bindings);
            }
            print_bindings(&bindings);
            Ok(())
        }
        OrchestrationCommands::Show { id } => {
            let binding = client.fetch_orchestration(*id).await?;
            if json_output {
                return output::json(&binding);
            }
            print_binding(&binding);
            Ok(())
        }
        OrchestrationCommands::Timeline { id } => {
            let events = client.fetch_orchestration_events(*id).await?;
            if json_output {
                return output::json(&events);
            }
            print_timeline(&events);
            Ok(())
        }
        OrchestrationCommands::Watch { id, interval } => {
            let interval = Duration::from_secs((*interval).max(WATCH_INTERVAL_MINIMUM));
            loop {
                let binding = client.fetch_orchestration(*id).await?;
                if json_output {
                    output::json(&binding)?;
                } else {
                    print_binding(&binding);
                }
                if binding.status.is_terminal() {
                    return Ok(());
                }
                time::sleep(interval).await;
            }
        }
        OrchestrationCommands::Intent {
            id,
            name,
            reason,
            payload,
            idempotency_key,
        } => {
            let payload = match payload {
                Some(path) => serde_json::from_slice(&fs::read(path)?)?,
                None => json!({}),
            };
            let key = idempotency_key
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let response = client
                .send_orchestration_intent(*id, name, payload, reason, &key)
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!("accepted intent {name} for orchestration {id} [{key}]");
            Ok(())
        }
        OrchestrationCommands::Requeue {
            id,
            reason,
            idempotency_key,
        } => {
            let key = idempotency_key
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let binding = client.requeue_orchestration(*id, reason, &key).await?;
            if json_output {
                return output::json(&binding);
            }
            println!(
                "requeued orchestration {} as generation {} ({})",
                binding.id, binding.generation, key
            );
            Ok(())
        }
        OrchestrationCommands::Adapters { command } => {
            orchestration_adapters(client, command, json_output).await
        }
    }
}

async fn orchestration_adapters(
    client: &Client,
    command: &OrchestrationAdapterCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        OrchestrationAdapterCommands::Kinds => {
            let kinds = client.fetch_orchestration_adapter_kinds().await?;
            if json_output {
                return output::json(&kinds);
            }
            let rows = kinds
                .into_iter()
                .map(|kind| {
                    vec![
                        kind.kind,
                        kind.version,
                        kind.display_name,
                        kind.capabilities.join(","),
                        kind.event_names.join(","),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(
                    &["KIND", "VERSION", "NAME", "CAPABILITIES", "EVENTS"],
                    &rows
                )
            );
            Ok(())
        }
        OrchestrationAdapterCommands::List => {
            let adapters = client.fetch_orchestration_adapters().await?;
            if json_output {
                return output::json(&adapters);
            }
            let rows = adapters
                .into_iter()
                .map(|adapter| {
                    vec![
                        adapter.id.to_string(),
                        adapter.name,
                        adapter.kind,
                        adapter.current_revision.to_string(),
                        adapter.enabled.to_string(),
                        adapter.endpoint_identity,
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(
                    &["ID", "NAME", "KIND", "REVISION", "ENABLED", "ENDPOINT"],
                    &rows,
                )
            );
            Ok(())
        }
        OrchestrationAdapterCommands::Show { id } => {
            let adapter = client.fetch_orchestration_adapter(*id).await?;
            let revisions = client.fetch_orchestration_adapter_revisions(*id).await?;
            let value = json!({ "adapter": adapter, "revisions": revisions });
            output::json(&value)
        }
        OrchestrationAdapterCommands::Apply { file, id } => {
            let definition: Value = serde_json::from_slice(&fs::read(file)?)?;
            let adapter = client.apply_orchestration_adapter(*id, &definition).await?;
            if json_output {
                return output::json(&adapter);
            }
            println!(
                "applied adapter {} ({}) revision {}",
                adapter.name, adapter.id, adapter.current_revision
            );
            Ok(())
        }
        OrchestrationAdapterCommands::Test { id, file } => {
            let sample: Value = serde_json::from_slice(&fs::read(file)?)?;
            let result = client.test_orchestration_adapter(*id, &sample).await?;
            output::json(&result)
        }
        OrchestrationAdapterCommands::Delete { id } => {
            let result = client.delete_orchestration_adapter(*id).await?;
            if json_output {
                return output::json(&result);
            }
            println!("{}", result.message);
            Ok(())
        }
        OrchestrationAdapterCommands::Reload => {
            let result = client.reload_orchestration_adapters().await?;
            output::json(&result)
        }
    }
}

fn print_bindings(bindings: &[OrchestrationBinding]) {
    let rows = bindings
        .iter()
        .map(|binding| {
            vec![
                binding.id.to_string(),
                binding.status.as_str().to_string(),
                binding.pipeline_id.to_string(),
                binding.scope.clone(),
                binding.correlation_key.clone(),
                binding.generation.to_string(),
                binding.current_epoch.to_string(),
                binding.current_phase.clone().unwrap_or_else(|| "-".into()),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &[
                "ID",
                "STATUS",
                "PIPELINE",
                "SCOPE",
                "CORRELATION",
                "GEN",
                "EPOCH",
                "PHASE"
            ],
            &rows,
        )
    );
}

fn print_binding(binding: &OrchestrationBinding) {
    println!("orchestration: {}", binding.id);
    println!("status: {}", binding.status.as_str());
    println!("pipeline: {}", binding.pipeline_id);
    println!("correlation: {}/{}", binding.scope, binding.correlation_key);
    println!("generation: {}", binding.generation);
    println!(
        "revision: {} ({})",
        binding.pipeline_revision, binding.pipeline_digest
    );
    println!("epoch: {}", binding.current_epoch);
    println!("phase: {}", binding.current_phase.as_deref().unwrap_or("-"));
    println!("attempt: {}", binding.current_attempt);
    println!("version: {}", binding.version);
}

fn print_timeline(events: &[OrchestrationEventReduction]) {
    let rows = events
        .iter()
        .map(|event| {
            vec![
                event.sequence.to_string(),
                event.winner.clone().unwrap_or_else(|| "-".into()),
                event.suppressed_intents.join(","),
                event.disposition.clone(),
                event.binding_version.to_string(),
                output::time(Some(event.created_at)),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &[
                "SEQ",
                "WINNER",
                "SUPPRESSED",
                "DISPOSITION",
                "VERSION",
                "CREATED"
            ],
            &rows,
        )
    );
}
