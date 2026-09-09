use super::*;

use runinator_models::workflow_vm::{WorkflowEffectOutput, WorkflowEffectOutputEvent};

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
            print_artifacts(&artifacts);
        }
    }
    Ok(())
}

pub(super) fn print_artifacts(events: &[WorkflowEffectOutputEvent]) {
    let rows = events
        .iter()
        .filter_map(|event| match &event.output {
            WorkflowEffectOutput::Artifact { artifact } => Some(vec![
                event.effect_id.to_string(),
                artifact.to_string(),
                event.created_at.to_string(),
            ]),
            _ => None,
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(&["effect_id", "artifact", "created_at"], &rows)
    );
}
