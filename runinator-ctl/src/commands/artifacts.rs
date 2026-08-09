use super::*;

pub(super) async fn artifacts(
    client: &Client,
    command: &ArtifactCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ArtifactCommands::List { node_run_id } => {
            let artifacts = client
                .fetch_workflow_node_run_artifacts(*node_run_id)
                .await?;
            if json_output {
                return output::json(&artifacts);
            }
            if artifacts.is_empty() {
                println!("no artifacts for node run {node_run_id}");
                return Ok(());
            }
            for artifact in artifacts {
                println!(
                    "{}\t{}\t{}\t{} bytes",
                    artifact.id, artifact.name, artifact.mime_type, artifact.size_bytes
                );
            }
        }
        ArtifactCommands::Download { id, out } => {
            let bytes = client.download_artifact(*id).await?;
            let path = out.clone().unwrap_or_else(|| PathBuf::from(id.to_string()));
            fs::write(&path, &bytes)?;
            if json_output {
                return output::json(
                    &json!({ "path": path.display().to_string(), "bytes": bytes.len() }),
                );
            }
            println!("wrote {} bytes to {}", bytes.len(), path.display());
        }
    }
    Ok(())
}
