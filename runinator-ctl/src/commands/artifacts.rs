use super::*;

pub(super) async fn artifacts(
    client: &Client,
    command: &ArtifactCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        ArtifactCommands::List { effect_id } => {
            let artifacts = client.fetch_workflow_effect_output(*effect_id).await?;
            if json_output {
                return output::json(&artifacts);
            }
            if artifacts.is_empty() {
                println!("no artifacts for effect {effect_id}");
                return Ok(());
            }
            for event in artifacts {
                if let runinator_models::workflow_vm::WorkflowEffectOutput::Artifact { artifact } =
                    event.output
                {
                    println!("{}", serde_json::to_string(&artifact)?);
                }
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
