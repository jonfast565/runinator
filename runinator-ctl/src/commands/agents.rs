//! externally hosted agent enrollment-token administration.

use std::collections::BTreeMap;

use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord};
use runinator_models::auth::CreateAgentEnrollmentTokenRequest;

use super::*;

pub(super) async fn agents(
    client: &Client,
    command: &AgentCommands,
    default_service_url: &str,
    json_output: bool,
) -> Result<()> {
    match command {
        AgentCommands::Diagnostics { replica_id } => {
            issue_and_print(
                client,
                *replica_id,
                AgentDirectiveKind::Diagnostics,
                json_output,
            )
            .await
        }
        AgentCommands::Drain { replica_id } => {
            issue_and_print(client, *replica_id, AgentDirectiveKind::Drain, json_output).await
        }
        AgentCommands::Restart { replica_id } => {
            issue_and_print(
                client,
                *replica_id,
                AgentDirectiveKind::Restart,
                json_output,
            )
            .await
        }
        AgentCommands::Logs { replica_id, lines } => {
            issue_and_print(
                client,
                *replica_id,
                AgentDirectiveKind::TailLogs { lines: *lines },
                json_output,
            )
            .await
        }
        AgentCommands::Directives { replica_id, limit } => {
            let records = client
                .list_agent_directives(*replica_id, Some(*limit))
                .await?;
            print_directives(&records, json_output)
        }
        AgentCommands::EnrollToken {
            ttl,
            labels,
            org,
            service_url,
            cluster_id,
            spki_pin,
        } => {
            let response = client
                .create_agent_enrollment_token(&CreateAgentEnrollmentTokenRequest {
                    ttl_seconds: parse_ttl(ttl)?,
                    org_id: *org,
                    labels: parse_labels(labels)?,
                    service_url: service_url
                        .clone()
                        .unwrap_or_else(|| default_service_url.to_string()),
                    cluster_id: *cluster_id,
                    spki_pin: spki_pin.clone(),
                })
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.token);
            eprintln!("This single-use enrollment token will not be shown again.");
            Ok(())
        }
        AgentCommands::EnrollmentTokens => {
            let tokens = client.list_agent_enrollment_tokens().await?;
            if json_output {
                return output::json(&tokens);
            }
            println!(
                "{:<14} {:<10} {:<25} labels",
                "token_id", "state", "expires_at"
            );
            for token in tokens {
                let state = if token.consumed_at.is_some() {
                    "consumed"
                } else if token.expires_at < Utc::now() {
                    "expired"
                } else {
                    "active"
                };
                let labels = token
                    .labels
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "{:<14} {:<10} {:<25} {}",
                    token.token_id,
                    state,
                    token.expires_at.to_rfc3339(),
                    labels
                );
            }
            Ok(())
        }
        AgentCommands::RevokeToken { token_id } => {
            let response = client.delete_agent_enrollment_token(token_id).await?;
            if json_output {
                return output::json(&response);
            }
            println!("{}", response.message);
            Ok(())
        }
    }
}

async fn issue_and_print(
    client: &Client,
    replica_id: uuid::Uuid,
    kind: AgentDirectiveKind,
    json_output: bool,
) -> Result<()> {
    let record = client
        .create_agent_directive(replica_id, &kind, Some(300))
        .await?;
    if json_output {
        return output::json(&record);
    }
    println!("{}\t{:?}", record.directive_id, record.state);
    Ok(())
}

fn print_directives(records: &[AgentDirectiveRecord], json_output: bool) -> Result<()> {
    if json_output {
        return output::json(&records.to_vec());
    }
    println!(
        "{:<38} {:<14} {:<25} message",
        "directive_id", "state", "issued_at"
    );
    for record in records {
        println!(
            "{:<38} {:<14} {:<25} {}",
            record.directive_id,
            format!("{:?}", record.state).to_ascii_lowercase(),
            record.issued_at.to_rfc3339(),
            record.message.as_deref().unwrap_or_default()
        );
    }
    Ok(())
}

fn parse_ttl(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    let split = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    let (amount, unit) = raw.split_at(split);
    let amount = amount
        .parse::<u64>()
        .map_err(|_| err(format!("invalid --ttl '{raw}'")))?;
    let multiplier = match unit {
        "s" | "" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => {
            return Err(err(format!(
                "invalid --ttl '{raw}', expected s, m, h, or d"
            )));
        }
    };
    amount
        .checked_mul(multiplier)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| err("--ttl must be greater than zero"))
}

fn parse_labels(labels: &[String]) -> Result<BTreeMap<String, String>> {
    let mut parsed = BTreeMap::new();
    for label in labels {
        let (key, value) = label
            .split_once('=')
            .filter(|(key, value)| !key.is_empty() && !value.is_empty())
            .ok_or_else(|| err(format!("invalid --label '{label}', expected KEY=VALUE")))?;
        parsed.insert(key.to_string(), value.to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
