//! reading the fleet: which runtimes have registered, what they are, and how they are doing.
//!
//! read-only on purpose. a replica row is a report *from* a runtime — the way to change one is to
//! scale a node group (`nodes`), direct an agent (`agents`), or stop the process itself.

use super::*;

use runinator_models::replicas::{ReplicaKind, ReplicaRecord, ReplicaStatus};
use runinator_models::telemetry::ReplicaSample;

use crate::cli::ReplicaCommands;

pub(super) async fn replicas(
    client: &Client,
    command: &ReplicaCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ReplicaCommands::List { kind, status, live } => {
            // `--live` is the filter almost every use of this wants, so it is a flag of its own
            // rather than something to remember the spelling of.
            let status = status
                .map(|status| status.into())
                .or_else(|| live.then_some(ReplicaStatus::Live));
            let replicas = fetch(client, kind.map(Into::into), status).await?;
            if json_output {
                return output::json(&replicas);
            }
            print_replicas(&replicas);
            Ok(())
        }
        ReplicaCommands::Ids { kind, status } => {
            let replicas = fetch(client, kind.map(Into::into), status.map(Into::into)).await?;
            if json_output {
                return output::json(
                    &replicas
                        .iter()
                        .map(|replica| replica.replica_id)
                        .collect::<Vec<_>>(),
                );
            }
            for replica in &replicas {
                println!("{}", replica.replica_id);
            }
            Ok(())
        }
        ReplicaCommands::Show { replica_id } => {
            // there is no fetch-one endpoint, and adding one for a list this size would be a route
            // that only saves a filter.
            let replicas = fetch(client, None, None).await?;
            let replica = replicas
                .into_iter()
                .find(|replica| replica.replica_id == *replica_id)
                .ok_or_else(|| err(format!("replica {replica_id} not found")))?;
            if json_output {
                return output::json(&replica);
            }
            print_replica(&replica)
        }
        ReplicaCommands::Providers { replica_id } => {
            let registrations = client.fetch_replica_providers(*replica_id).await?;
            if json_output {
                return output::json(&registrations);
            }
            let rows = registrations
                .iter()
                .map(|registration| {
                    vec![
                        output::truncate(&registration.provider_name, 28),
                        registration.provider.actions.len().to_string(),
                        output::truncate(
                            &registration.provider.metadata.credential_scopes.join(","),
                            36,
                        ),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(&["provider", "actions", "credential_scopes"], &rows)
            );
            Ok(())
        }
        ReplicaCommands::Samples {
            replica_id,
            since_seconds,
            limit,
        } => {
            let series = client
                .fetch_replica_samples(*replica_id, *since_seconds)
                .await?;
            if json_output {
                return output::json(&series);
            }
            let start = series.samples.len().saturating_sub(*limit);
            print_samples(&series.samples[start..]);
            Ok(())
        }
    }
}

async fn fetch(
    client: &Client,
    kind: Option<ReplicaKind>,
    status: Option<ReplicaStatus>,
) -> Result<Vec<ReplicaRecord>> {
    Ok(client.fetch_replicas(kind, status).await?.replicas)
}

fn print_replicas(replicas: &[ReplicaRecord]) {
    let rows = replicas
        .iter()
        .map(|replica| {
            vec![
                replica.replica_id.to_string(),
                replica.replica_type.as_str().to_string(),
                replica.status.as_str().to_string(),
                output::truncate(name(replica), 28),
                output::truncate(&endpoint(replica), 32),
                replica.last_heartbeat_at.to_rfc3339(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &["id", "kind", "status", "name", "endpoint", "last_heartbeat"],
            &rows
        )
    );
}

fn print_replica(replica: &ReplicaRecord) -> Result<()> {
    println!("id: {}", replica.replica_id);
    println!("kind: {}", replica.replica_type.as_str());
    println!("status: {}", replica.status.as_str());
    println!("name: {}", name(replica));
    println!("instance: {}", replica.instance_id);
    println!("runtime: {}", replica.runtime_id);
    println!("endpoint: {}", endpoint(replica));
    if let Some(ip) = &replica.observed_ip {
        println!("observed_ip: {ip}");
    }
    if let Some(version) = &replica.version {
        println!("version: {version}");
    }
    println!("first_seen_at: {}", replica.first_seen_at.to_rfc3339());
    println!(
        "last_heartbeat_at: {}",
        replica.last_heartbeat_at.to_rfc3339()
    );
    println!("offline_at: {}", output::time(replica.offline_at));
    if !replica.attributes.is_null() {
        println!(
            "attributes: {}",
            serde_json::to_string_pretty(&replica.attributes)?
        );
    }
    Ok(())
}

fn print_samples(samples: &[ReplicaSample]) {
    let rows = samples
        .iter()
        .map(|sample| {
            vec![
                sample.sampled_at.to_rfc3339(),
                format!("{:.1}", sample.cpu_percent),
                format!("{:.1}", sample.mem_percent),
                format!("{:.1}", sample.process_cpu_percent),
                bytes(sample.process_mem_bytes),
                sample
                    .load_one
                    .map(|load| format!("{load:.2}"))
                    .unwrap_or_else(|| "-".into()),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(
            &[
                "sampled_at",
                "cpu%",
                "mem%",
                "proc_cpu%",
                "proc_mem",
                "load1"
            ],
            &rows
        )
    );
}

// a replica names itself when it can; an unnamed one is still addressable by its instance.
fn name(replica: &ReplicaRecord) -> &str {
    replica
        .display_name
        .as_deref()
        .unwrap_or(&replica.instance_id)
}

fn endpoint(replica: &ReplicaRecord) -> String {
    match (&replica.host, replica.port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.clone(),
        _ => "-".into(),
    }
}

fn bytes(value: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1}{}", UNITS[unit])
}
