use super::*;

pub(super) async fn status(client: &Client, json_output: bool) -> Result<()> {
    let workflows = match client.fetch_workflows().await {
        Ok(workflows) => workflows,
        Err(err) => {
            if json_output {
                return output::json(&json!({
                    "api": { "reachable": false, "error": err.to_string() }
                }));
            }
            println!("api: unreachable");
            println!("error: {err}");
            return Ok(());
        }
    };
    let supervisor = match client.fetch_supervisor_status().await {
        Ok(value) => value,
        Err(err) => json!({ "configured": false, "error": err.to_string() }),
    };
    let mut counts = Map::new();
    for status in non_terminal_statuses() {
        let runs = client.fetch_workflow_runs_by_status(status).await?;
        counts.insert(status.as_str().into(), runs.len().into());
    }

    if json_output {
        return output::json(&json!({
            "api": { "reachable": true, "workflow_count": workflows.len() },
            "supervisor": supervisor,
            "workflow_runs": counts
        }));
    }

    println!("api: reachable");
    println!("workflows: {}", workflows.len());
    match supervisor.get("configured").and_then(Value::as_bool) {
        Some(true) => {
            let stale = supervisor
                .get("stale_seconds")
                .and_then(Value::as_i64)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into());
            println!("supervisor: configured, stale_seconds={stale}");
        }
        _ => println!("supervisor: unavailable"),
    }
    println!();
    println!("{:<18} {:>6}", "status", "runs");
    for (status, count) in counts {
        println!("{:<18} {:>6}", status, count.as_u64().unwrap_or_default());
    }
    Ok(())
}
