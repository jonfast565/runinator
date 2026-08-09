use super::*;

pub(super) async fn nodes(
    client: &Client,
    command: &NodeCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        NodeCommands::List => {
            let backends = client.fetch_node_backends().await?;
            let groups = client.fetch_nodes().await?;
            if json_output {
                return output::json(&json!({
                    "backends": backends.backends,
                    "groups": groups,
                }));
            }
            if backends.backends.is_empty() {
                println!("no provisioning backends configured");
                return Ok(());
            }
            println!("{:<12} {:<10} {:<10}", "backend", "available", "kinds");
            for backend in &backends.backends {
                let kinds = backend
                    .kinds
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "{:<12} {:<10} {:<10}",
                    backend.backend.as_str(),
                    backend.available,
                    kinds
                );
            }
            println!();
            print_groups(&groups);
            Ok(())
        }
        NodeCommands::SpinUp {
            backend,
            kind,
            count,
            labels,
        } => {
            let backend = (*backend).into();
            let kind: ReplicaKind = (*kind).into();
            let current = client
                .fetch_nodes()
                .await?
                .into_iter()
                .find(|group| group.backend == backend && group.kind == kind)
                .map(|group| group.desired)
                .unwrap_or(0);
            let spec = NodeSpec {
                labels: parse_labels(labels)?,
                ..Default::default()
            };
            let group = client
                .scale_nodes(&ScaleNodesRequest {
                    backend,
                    kind,
                    desired: current + *count,
                    spec,
                })
                .await?;
            report_group(&group, json_output)
        }
        NodeCommands::Scale {
            backend,
            kind,
            desired,
        } => {
            let group = client
                .scale_nodes(&ScaleNodesRequest {
                    backend: (*backend).into(),
                    kind: (*kind).into(),
                    desired: *desired,
                    spec: NodeSpec::default(),
                })
                .await?;
            report_group(&group, json_output)
        }
        NodeCommands::Stop { backend, node } => {
            let value = client
                .stop_node(&StopNodeRequest {
                    backend: (*backend).into(),
                    node_id: node.clone(),
                })
                .await?;
            if json_output {
                return output::json(&value);
            }
            println!("stopped node {node}");
            Ok(())
        }
    }
}

fn parse_labels(labels: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
    let mut map = std::collections::BTreeMap::new();
    for entry in labels {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| err(format!("invalid --label '{entry}', expected KEY=VALUE")))?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn print_groups(groups: &[ProvisionedGroup]) {
    if groups.is_empty() {
        println!("no node groups");
        return;
    }
    println!(
        "{:<12} {:<10} {:<8} {:<8} {:<9} {:<12}",
        "backend", "kind", "desired", "live", "managed", "name"
    );
    for group in groups {
        println!(
            "{:<12} {:<10} {:<8} {:<8} {:<9} {:<12}",
            group.backend.as_str(),
            group.kind.as_str(),
            group.desired,
            group.available,
            // ghost rows are kinds this backend has no template/deployment for.
            if group.manageable { "yes" } else { "no" },
            group.name
        );
    }
}

fn report_group(group: &ProvisionedGroup, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(group);
    }
    println!(
        "{} {} on {}: desired={} live={}",
        group.kind.as_str(),
        group.name,
        group.backend.as_str(),
        group.desired,
        group.available
    );
    Ok(())
}
