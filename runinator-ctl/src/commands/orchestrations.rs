use super::*;

use runinator_models::orchestration::{OrchestrationBinding, OrchestrationEventReduction};

use crate::cli::OrchestrationCommands;

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
